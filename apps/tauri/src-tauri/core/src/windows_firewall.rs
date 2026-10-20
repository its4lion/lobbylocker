//! Native Windows Firewall access. The structured helper supplies elevation;
//! neither inspection nor mutation starts a shell or another firewall tool.
use crate::{
    firewall::{
        replace_windows_rules, windows_rule_plan, Request, WindowsRuleSpec, WindowsRuleStore,
        WINDOWS_GROUP,
    },
    inspection::{
        self, Inspection, InspectionError, InspectionResult, VerificationIssue, MAX_READBACK_BYTES,
    },
    Result,
};
use serde_json::{json, Value};
use std::{collections::BTreeSet, time::Instant};
use windows::{
    core::{w, Interface, BSTR, PCWSTR},
    Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, RPC_E_CHANGED_MODE, VARIANT_TRUE,
        },
        Management::MobileDeviceManagementRegistration::IsDeviceRegisteredWithManagement,
        NetworkManagement::WindowsFirewall::{
            INetFwPolicy2, INetFwRule, INetFwRule3, INetFwRules, NetFwPolicy2, NetFwRule,
            NET_FW_ACTION_BLOCK, NET_FW_MODIFY_STATE_INBOUND_BLOCKED, NET_FW_MODIFY_STATE_OK,
            NET_FW_PROFILE2_ALL, NET_FW_PROFILE2_DOMAIN, NET_FW_PROFILE2_PRIVATE,
            NET_FW_PROFILE2_PUBLIC, NET_FW_RULE_DIR_OUT,
        },
        System::{
            Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            },
            Ole::{IEnumVARIANT, SafeArrayGetDim, SafeArrayGetLBound, SafeArrayGetUBound},
            Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_SUBKEY_WOW6464KEY},
            Services::{
                CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_HANDLE,
                SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS,
            },
            Variant::{
                VariantClear, VARIANT, VT_ARRAY, VT_BYREF, VT_DISPATCH, VT_EMPTY, VT_NULL,
                VT_UNKNOWN,
            },
        },
    },
};

fn native<T>(operation: &str, result: windows::core::Result<T>) -> InspectionResult<T> {
    result.map_err(|error| {
        inspection::windows_hresult_error(error.code().0, format!("{operation}: {error}"))
    })
}

struct Apartment(bool);

impl Apartment {
    fn enter() -> InspectionResult<Self> {
        // Tauri workers normally have no apartment. An existing MTA apartment
        // can use these automation interfaces without changing its model.
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        native("Could not initialize Windows firewall access", result.ok())?;
        Ok(Self(true))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

struct Variant(VARIANT);

impl Drop for Variant {
    fn drop(&mut self) {
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}

impl Variant {
    fn unrestricted(&self) -> InspectionResult<bool> {
        let value = unsafe { &self.0.Anonymous.Anonymous };
        if matches!(value.vt, VT_EMPTY | VT_NULL) {
            return Ok(true);
        }
        // A BYREF variant contains a pointer to the array pointer and cannot be
        // interpreted as a directly owned SAFEARRAY. Refuse unknown forms.
        if value.vt.0 & VT_ARRAY.0 != 0 && value.vt.0 & VT_BYREF.0 == 0 {
            let array = unsafe { value.Anonymous.parray };
            if !array.is_null() {
                let dimensions = unsafe { SafeArrayGetDim(array) };
                for dimension in 1..=dimensions {
                    let lower = native("Could not inspect firewall interface filters", unsafe {
                        SafeArrayGetLBound(array, dimension)
                    })?;
                    let upper = native("Could not inspect firewall interface filters", unsafe {
                        SafeArrayGetUBound(array, dimension)
                    })?;
                    if upper < lower {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}

fn policy() -> InspectionResult<INetFwPolicy2> {
    native("Could not open Windows Firewall", unsafe {
        CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)
    })
}

struct OwnedRule {
    name: String,
    rule: INetFwRule,
}

/// Every returned rule belongs to the managed group. Also check names across
/// groups before removal, because INetFwRules.Remove accepts only a name.
fn owned_rules(rules: &INetFwRules, removing: bool) -> InspectionResult<Vec<OwnedRule>> {
    let unknown = native("Could not enumerate Windows firewall rules", unsafe {
        rules._NewEnum()
    })?;
    let enumerator: IEnumVARIANT = native("Invalid firewall rule enumerator", unknown.cast())?;
    let mut owned = Vec::new();
    let mut foreign_names = BTreeSet::new();
    let started = Instant::now();
    let mut count = 0usize;
    loop {
        // VARIANT owns the enumerator's reference until the end of this loop;
        // casting below takes its own reference. Always clear it on failure.
        let mut value = Variant(VARIANT::default());
        let mut fetched = 0;
        native("Could not read Windows firewall rules", unsafe {
            enumerator
                .Next(std::slice::from_mut(&mut value.0), &mut fetched)
                .ok()
        })?;
        if fetched == 0 {
            break;
        }
        count += 1;
        if count > 100_000 || started.elapsed().as_secs() >= 15 {
            return Err("Windows firewall rule enumeration exceeded its safety limit.".into());
        }
        let data = unsafe { &value.0.Anonymous.Anonymous };
        let rule: INetFwRule = match data.vt {
            VT_DISPATCH => native("Invalid Windows firewall rule object", unsafe {
                data.Anonymous
                    .pdispVal
                    .as_ref()
                    .ok_or("Missing firewall rule object.")?
                    .cast()
            })?,
            VT_UNKNOWN => native("Invalid Windows firewall rule object", unsafe {
                data.Anonymous
                    .punkVal
                    .as_ref()
                    .ok_or("Missing firewall rule object.")?
                    .cast()
            })?,
            _ => return Err("Unsupported Windows firewall rule object.".into()),
        };
        let group = native("Could not read firewall rule ownership", unsafe {
            rule.Grouping()
        })?;
        if group == WINDOWS_GROUP {
            let name =
                native("Could not read firewall rule name", unsafe { rule.Name() })?.to_string();
            owned.push(OwnedRule { name, rule });
            if owned.len() > 10_000 {
                return Err("Too many LobbyLocker firewall rules to verify.".into());
            }
        } else if removing {
            foreign_names.insert(
                native("Could not read firewall rule name", unsafe { rule.Name() })?
                    .to_string()
                    .to_lowercase(),
            );
        }
    }
    if removing
        && owned
            .iter()
            .any(|rule| foreign_names.contains(&rule.name.to_lowercase()))
    {
        return Err("A LobbyLocker firewall rule shares its name with another application's rule. Refusing to remove either rule.".into());
    }
    Ok(owned)
}

fn create_rule(spec: &WindowsRuleSpec) -> InspectionResult<INetFwRule> {
    let rule: INetFwRule = native("Could not create a Windows firewall rule", unsafe {
        CoCreateInstance(&NetFwRule, None, CLSCTX_INPROC_SERVER)
    })?;
    // Protocol precedes ports as required by INetFwRule. Set all fields before
    // Add so incomplete rules never enter the live collection.
    native("Could not configure a Windows firewall rule", unsafe {
        (|| -> windows::core::Result<()> {
            rule.SetName(&BSTR::from(&spec.name))?;
            rule.SetDescription(&BSTR::from(format!("LobbyLocker: {}", spec.source_id)))?;
            rule.SetGrouping(&BSTR::from(WINDOWS_GROUP))?;
            if let Some(program) = &spec.program {
                rule.SetApplicationName(&BSTR::from(program))?;
            }
            rule.SetProtocol(spec.protocol)?;
            rule.SetRemoteAddresses(&BSTR::from(&spec.address))?;
            if let Some(ports) = &spec.ports {
                rule.SetRemotePorts(&BSTR::from(ports))?;
            }
            rule.SetDirection(NET_FW_RULE_DIR_OUT)?;
            rule.SetAction(NET_FW_ACTION_BLOCK)?;
            rule.SetProfiles(NET_FW_PROFILE2_ALL.0)?;
            rule.SetInterfaceTypes(&BSTR::from("All"))?;
            rule.SetEnabled(VARIANT_TRUE)?;
            Ok(())
        })()
    })?;
    Ok(rule)
}

struct NativeStore(INetFwRules);

impl WindowsRuleStore for NativeStore {
    fn managed_names(&mut self) -> Result<Vec<String>> {
        owned_rules(&self.0, true)
            .map(|rules| rules.into_iter().map(|rule| rule.name).collect())
            .map_err(|error| error.to_string())
    }

    fn add(&mut self, spec: &WindowsRuleSpec) -> Result<()> {
        // Add can replace an existing rule with the same identifier. Never
        // overwrite a foreign object, even if a generated name was preclaimed.
        match unsafe { self.0.Item(&BSTR::from(&spec.name)) } {
            Ok(_) => return Err("A firewall rule already uses a replacement rule's name.".into()),
            Err(error) if matches!(error.code().0 as u32, 0x80070002 | 0x80070490) => {}
            Err(error) => {
                return Err(inspection::windows_hresult_error(
                    error.code().0,
                    format!("Could not check a replacement firewall rule's name: {error}"),
                )
                .to_string());
            }
        }
        let rule = create_rule(spec).map_err(|error| error.to_string())?;
        native("Could not add a Windows firewall rule", unsafe {
            self.0.Add(&rule)
        })
        .map_err(|error| error.to_string())
    }

    fn remove(&mut self, name: &str) -> Result<()> {
        // Recheck ownership immediately before mutation. The initial full
        // enumeration also refuses name collisions across application groups.
        let rule = match unsafe { self.0.Item(&BSTR::from(name)) } {
            Ok(rule) => rule,
            // Legacy PowerShell rules can share a friendly Name for TCP and
            // UDP. If Remove deleted both, the repeated removal is complete.
            Err(error) if matches!(error.code().0 as u32, 0x80070002 | 0x80070490) => return Ok(()),
            Err(error) => {
                return Err(inspection::windows_hresult_error(
                    error.code().0,
                    format!("Could not find a LobbyLocker firewall rule for removal: {error}"),
                )
                .to_string());
            }
        };
        let group = native("Could not verify firewall rule ownership", unsafe {
            rule.Grouping()
        })
        .map_err(|error| error.to_string())?;
        if group != WINDOWS_GROUP {
            return Err("Firewall rule ownership changed. Refusing to remove it.".into());
        }
        native("Could not remove a LobbyLocker firewall rule", unsafe {
            self.0.Remove(&BSTR::from(name))
        })
        .map_err(|error| error.to_string())
    }
}

pub(crate) fn execute(request: &Request) -> Result<()> {
    let plan = windows_rule_plan(request)?;
    // Revalidate inside the privileged helper before creating or deleting any
    // rules. It receives structured input, never paths from an unvalidated shell.
    if let Request::Apply { rules } = request {
        crate::model::validate_program_files(rules)?;
    }
    let _apartment = Apartment::enter().map_err(|error| error.to_string())?;
    let policy = policy().map_err(|error| error.to_string())?;
    let rules = native("Could not open Windows firewall rules", unsafe {
        policy.Rules()
    })
    .map_err(|error| error.to_string())?;
    replace_windows_rules(&mut NativeStore(rules), &plan)
}

struct ServiceHandle(SC_HANDLE);

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn services_running() -> InspectionResult<bool> {
    let manager = ServiceHandle(native(
        "Could not inspect Windows firewall services",
        unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) },
    )?);
    for name in [w!("MpsSvc"), w!("BFE")] {
        let service = ServiceHandle(native(
            "Could not inspect a Windows firewall service",
            unsafe { OpenServiceW(manager.0, name, SERVICE_QUERY_STATUS) },
        )?);
        let mut status = SERVICE_STATUS::default();
        native(
            "Could not inspect Windows firewall service status",
            unsafe { QueryServiceStatus(service.0, &mut status) },
        )?;
        if status.dwCurrentState != SERVICE_RUNNING {
            return Ok(false);
        }
    }
    Ok(true)
}

fn group_policy_flag(profile: &str, setting: &str) -> InspectionResult<Option<bool>> {
    let path: Vec<u16> = format!(r"SOFTWARE\Policies\Microsoft\WindowsFirewall\{profile}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut value = 0u32;
    let mut length = std::mem::size_of_val(&value) as u32;
    let setting: Vec<u16> = setting.encode_utf16().chain(Some(0)).collect();
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(path.as_ptr()),
            PCWSTR(setting.as_ptr()),
            RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY,
            None,
            Some(std::ptr::from_mut(&mut value).cast()),
            Some(&mut length),
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    native(
        "Could not inspect Windows firewall group policy",
        status.ok(),
    )?;
    if length != 4 || value > 1 {
        return Err("Windows firewall group policy contains an invalid boolean setting. Effective protection cannot be verified.".into());
    }
    Ok(Some(value == 1))
}

fn group_policy_allows_rules(profile: &str) -> InspectionResult<bool> {
    // INetFwPolicy2.FirewallEnabled reports the local setting. Group policy can
    // disable enforcement independently, or prevent local rules from merging.
    for setting in ["EnableFirewall", "AllowLocalPolicyMerge"] {
        if group_policy_flag(profile, setting)? == Some(false) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn reject_unverified_mdm_policy() -> InspectionResult<()> {
    let mut enrolled = windows::core::BOOL::default();
    native(
        "Could not inspect Windows device-management registration",
        unsafe {
            // UPN is optional: registration status is sufficient and no account
            // details need to be collected or retained by LobbyLocker.
            IsDeviceRegisteredWithManagement(&mut enrolled, None)
        },
    )?;
    if enrolled.as_bool() {
        return Err(InspectionError::Issue(
            VerificationIssue::ManagedByDeviceManagement,
            "Windows device management may override local firewall rules. Effective MDM firewall policy cannot yet be verified by LobbyLocker.".into(),
        ));
    }
    Ok(())
}

fn enforced(policy: &INetFwPolicy2) -> InspectionResult<bool> {
    if !services_running()? {
        return Ok(false);
    }
    let modify_state = native("Could not inspect Windows firewall policy", unsafe {
        policy.LocalPolicyModifyState()
    })?;
    // INBOUND_BLOCKED concerns inbound exceptions; all managed rules are
    // outbound blocks. Refuse GPO overrides and unknown future policy states.
    if !matches!(
        modify_state,
        NET_FW_MODIFY_STATE_OK | NET_FW_MODIFY_STATE_INBOUND_BLOCKED
    ) {
        return Ok(false);
    }
    for (profile, name) in [
        (NET_FW_PROFILE2_DOMAIN, "DomainProfile"),
        (NET_FW_PROFILE2_PRIVATE, "PrivateProfile"),
        (NET_FW_PROFILE2_PUBLIC, "PublicProfile"),
    ] {
        if !native("Could not inspect Windows firewall profile", unsafe {
            policy.get_FirewallEnabled(profile)
        })?
        .as_bool()
            || !group_policy_allows_rules(name)?
            || !Variant(native(
                "Could not inspect excluded firewall interfaces",
                unsafe { policy.get_ExcludedInterfaces(profile) },
            )?)
            .unrestricted()?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn any(value: &str) -> bool {
    value.is_empty() || value == "*" || value.eq_ignore_ascii_case("any")
}

fn source_id(name: &str) -> Option<String> {
    // INetFwRule.Name exposes the old NetSecurity DisplayName. Older releases
    // used the same friendly name for a rule's TCP and UDP variants.
    if let Some(id) = name.strip_prefix("LobbyLocker: ") {
        return crate::model::validate_id(id).ok().map(|()| id.to_owned());
    }
    let mut segments = name.strip_prefix("LobbyLocker.")?.split('.');
    let transaction = segments.next()?;
    let id = segments.next()?;
    let protocol = segments.next()?;
    if transaction.len() != 32
        || !transaction.bytes().all(|b| b.is_ascii_hexdigit())
        || !matches!(protocol, "TCP" | "UDP" | "Any")
        || segments.next().is_some()
        || crate::model::validate_id(id).is_err()
    {
        return None;
    }
    Some(id.to_owned())
}

fn rule_row(owned: &OwnedRule) -> InspectionResult<Option<Value>> {
    let rule = &owned.rule;
    if !native("Could not inspect firewall rule state", unsafe {
        rule.Enabled()
    })?
    .as_bool()
        || native("Could not inspect firewall rule direction", unsafe {
            rule.Direction()
        })? != NET_FW_RULE_DIR_OUT
        || native("Could not inspect firewall rule action", unsafe {
            rule.Action()
        })? != NET_FW_ACTION_BLOCK
    {
        return Ok(None);
    }
    let protocol = native("Could not inspect firewall protocol", unsafe {
        rule.Protocol()
    })?;
    let rule3: INetFwRule3 = native("Unsupported Windows firewall rule interface", rule.cast())?;
    let supported = native("Could not inspect firewall profiles", unsafe {
        rule.Profiles()
    })? == NET_FW_PROFILE2_ALL.0
        && any(
            &native("Could not inspect local firewall addresses", unsafe {
                rule.LocalAddresses()
            })?
            .to_string(),
        )
        && any(
            &native("Could not inspect firewall service filter", unsafe {
                rule.ServiceName()
            })?
            .to_string(),
        )
        && native("Could not inspect firewall interface types", unsafe {
            rule.InterfaceTypes()
        })?
        .to_string()
        .eq_ignore_ascii_case("All")
        && Variant(native(
            "Could not inspect firewall interface filter",
            unsafe { rule.Interfaces() },
        )?)
        .unrestricted()?
        && any(
            &native("Could not inspect firewall package filter", unsafe {
                rule3.LocalAppPackageId()
            })?
            .to_string(),
        )
        && any(
            &native("Could not inspect firewall user-owner filter", unsafe {
                rule3.LocalUserOwner()
            })?
            .to_string(),
        )
        && any(&native("Could not inspect firewall user filter", unsafe {
            rule3.LocalUserAuthorizedList()
        })?
        .to_string())
        && any(
            &native("Could not inspect firewall remote-user filter", unsafe {
                rule3.RemoteUserAuthorizedList()
            })?
            .to_string(),
        )
        && any(
            &native("Could not inspect firewall remote-machine filter", unsafe {
                rule3.RemoteMachineAuthorizedList()
            })?
            .to_string(),
        )
        && native("Could not inspect firewall security filter", unsafe {
            rule3.SecureFlags()
        })? == 0;
    if !supported {
        return Err(InspectionError::Issue(
            VerificationIssue::UnsupportedFilters,
            "LobbyLocker firewall rules use filters that cannot be verified.".into(),
        ));
    }
    let ports = if matches!(protocol, 6 | 17) {
        if !any(&native("Could not inspect local firewall ports", unsafe {
            rule.LocalPorts()
        })?
        .to_string())
        {
            return Err("LobbyLocker rules have unsupported local port filters.".into());
        }
        native("Could not inspect remote firewall ports", unsafe {
            rule.RemotePorts()
        })?
        .to_string()
    } else {
        "*".into()
    };
    let ports = if any(&ports) { "Any".into() } else { ports };
    let program = native("Could not inspect firewall program", unsafe {
        rule.ApplicationName()
    })?
    .to_string();
    let program = if any(&program) { None } else { Some(program) };
    let address = native("Could not inspect remote firewall addresses", unsafe {
        rule.RemoteAddresses()
    })?
    .to_string();
    Ok(Some(
        json!({ "address": address, "protocol": protocol, "ports": ports, "program": program, "sourceId": source_id(&owned.name) }),
    ))
}

pub(crate) fn inspect() -> InspectionResult<Inspection> {
    let _apartment = Apartment::enter()?;
    let policy = policy()?;
    let rules = native("Could not open Windows firewall rules", unsafe {
        policy.Rules()
    })?;
    let mut rows = Vec::new();
    let mut bytes = 0usize;
    for rule in owned_rules(&rules, false)? {
        if let Some(row) = rule_row(&rule)? {
            bytes += row.to_string().len();
            if bytes > MAX_READBACK_BYTES {
                return Err("Firewall inspection response is too large.".into());
            }
            rows.push(row);
        }
    }
    let enforced = if rows.is_empty() {
        true
    } else {
        // The COM policy object exposes local settings, not every MDM policy
        // store. Do not turn configured local rules into active-block claims
        // on an enrolled device until effective MDM policy can be queried.
        reject_unverified_mdm_policy()?;
        enforced(&policy)?
    };
    // Reuse the established parser for canonical address, program, protocol,
    // ports, ownership attribution and effective-enforcement checks.
    inspection::windows_snapshot(&json!({ "enforced": enforced, "supported": true, "rules": rows }))
        .map_err(|failure| match failure.issue {
            Some(issue) => InspectionError::Issue(issue, failure.message),
            None => InspectionError::Other(failure.message),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_names_only_attribute_validated_ids() {
        let generation = "0123456789abcdef0123456789ABCDEF";
        assert_eq!(
            source_id(&format!("LobbyLocker.{generation}.game-eu.TCP")),
            Some("game-eu".into())
        );
        assert_eq!(
            source_id("LobbyLocker: legacy-game-eu"),
            Some("legacy-game-eu".into())
        );
        for name in [
            format!("Other.{generation}.game-eu.TCP"),
            "LobbyLocker.invalid.game-eu.TCP".into(),
            format!("LobbyLocker.{generation}.game-eu.TCP.extra"),
            format!("LobbyLocker.{generation}.bad id.TCP"),
            format!("LobbyLocker.{generation}.game-eu.ICMP"),
            "LobbyLocker: bad id".into(),
            "LobbyLocker: ".into(),
        ] {
            assert_eq!(source_id(&name), None);
        }
    }

    #[test]
    fn in_memory_rules_preserve_scope_and_reject_user_owner_filters() {
        let _apartment = Apartment::enter().unwrap();
        let spec = WindowsRuleSpec {
            name: "LobbyLocker.0123456789abcdef0123456789abcdef.game-eu.TCP".into(),
            source_id: "game-eu".into(),
            program: Some(r"C:\Games\Example\game.exe".into()),
            address: "192.0.2.1".into(),
            protocol: 6,
            ports: Some("443-443".into()),
        };
        // This object is never added to INetFwRules and cannot change the OS
        // firewall. It also checks the COM defaults used by our rule builder.
        let rule = create_rule(&spec).unwrap();
        let owned = OwnedRule {
            name: spec.name,
            rule,
        };
        let row = rule_row(&owned).unwrap().unwrap();
        assert_eq!(row["sourceId"], "game-eu");
        assert_eq!(row["program"].as_str(), spec.program.as_deref());
        let rule3: INetFwRule3 = owned.rule.cast().unwrap();
        unsafe { rule3.SetLocalUserOwner(&BSTR::from("S-1-5-21-1-2-3-1001")) }.unwrap();
        assert!(rule_row(&owned).is_err());
    }

    #[test]
    fn all_apps_rules_have_no_application_filter() {
        let _apartment = Apartment::enter().unwrap();
        let spec = WindowsRuleSpec {
            name: "LobbyLocker.0123456789abcdef0123456789abcdef.game-eu.UDP".into(),
            source_id: "game-eu".into(),
            program: None,
            address: "192.0.2.1".into(),
            protocol: 17,
            ports: None,
        };
        // In-memory COM object only, never added to the live policy store.
        let owned = OwnedRule {
            name: spec.name.clone(),
            rule: create_rule(&spec).unwrap(),
        };
        let row = rule_row(&owned).unwrap().unwrap();
        assert!(row["program"].is_null());
        assert_eq!(row["sourceId"], "game-eu");
    }
}

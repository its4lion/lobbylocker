(() => {
  const fallbackVersion = 'v0.0.11';
  const fallbackDownloads = {
    windows: 'https://github.com/its4lion/lobbylocker/releases/download/v0.0.11/LobbyLocker-0.0.11.exe',
    deb: 'https://github.com/its4lion/lobbylocker/releases/download/v0.0.11/release-assets-lobbylocker_0.0.11_amd64.deb',
    arch: 'https://github.com/its4lion/lobbylocker/releases/download/v0.0.11/lobbylocker-0.0.11-1-x86_64.pkg.tar.zst',
    appimage: 'https://github.com/its4lion/lobbylocker/releases/download/v0.0.11/LobbyLocker-0.0.11-x86_64.AppImage'
  };
  let downloads = { ...fallbackDownloads };
  let version = fallbackVersion;
  const links = [...document.querySelectorAll('[data-download]')];
  const osSwitch = document.querySelector('.os-switch');
  const osChoices = [...document.querySelectorAll('[data-os-choice]')];
  const panels = [...document.querySelectorAll('[data-os-panel]')];
  const notes = [...document.querySelectorAll('[data-os-note]')];
  let platform = /Linux/i.test(navigator.userAgent) && !/Android/i.test(navigator.userAgent) ? 'linux' : 'windows';

  function render() {
    document.querySelector('#release-version').textContent = version;
    osChoices.forEach((choice) => choice.setAttribute('aria-pressed', String(choice.dataset.osChoice === platform)));
    panels.forEach((panel) => panel.setAttribute('aria-hidden', String(panel.dataset.osPanel !== platform)));
    notes.forEach((note) => note.setAttribute('aria-hidden', String(note.dataset.osNote !== platform)));
    links.forEach((link) => {
      const url = downloads[link.dataset.download];
      const label = link.dataset.label || 'Download';
      const accessibleLabel = link.dataset.accessibleLabel || 'Download Windows installer';
      link.textContent = label;
      if (url) {
        link.href = url;
        link.removeAttribute('aria-disabled');
        link.removeAttribute('title');
        link.setAttribute('aria-label', accessibleLabel);
      } else {
        link.removeAttribute('href');
        link.setAttribute('aria-disabled', 'true');
        link.setAttribute('title', 'Unavailable for this release');
        link.setAttribute('aria-label', `${accessibleLabel} unavailable for this release`);
      }
    });
  }
  osChoices.forEach((choice) => choice.addEventListener('click', () => {
    if (choice.dataset.osChoice === 'windows' || choice.dataset.osChoice === 'linux') platform = choice.dataset.osChoice;
    render();
  }));
  document.querySelector('.download-stack').dataset.ready = 'true';
  document.querySelector('.download-notes').dataset.ready = 'true';
  osSwitch.hidden = false;
  render();

  fetch('https://api.github.com/repos/its4lion/LobbyLocker', { cache: 'no-cache' })
    .then((response) => {
      if (!response.ok) throw new Error('Repository lookup failed');
      return response.json();
    })
    .then((repository) => {
      const count = repository.stargazers_count;
      if (!Number.isSafeInteger(count) || count < 0) return;
      const stars = document.querySelector('#github-stars');
      stars.textContent = count.toLocaleString();
      stars.hidden = false;
      document.querySelector('#github-stars-link').setAttribute('aria-label', `LobbyLocker has ${count.toLocaleString()} stars on GitHub`);
    })
    .catch(() => { /* Keep the GitHub link if the count is unavailable. */ });

  // Only published stable releases from this repository may replace the fallback links.
  fetch('https://api.github.com/repos/its4lion/LobbyLocker/releases/latest', { cache: 'no-cache' })
    .then((response) => {
      if (!response.ok) throw new Error('Release lookup failed');
      return response.json();
    })
    .then((release) => {
      if (release.draft !== false || release.prerelease !== false || !/^v\d+\.\d+\.\d+$/.test(release.tag_name) || !Array.isArray(release.assets)) return;
      const number = release.tag_name.slice(1);
      const names = {
        windows: (name) => name.toLowerCase() === `lobbylocker-${number}.exe`,
        deb: (name) => name.toLowerCase() === `release-assets-lobbylocker_${number}_amd64.deb`,
        arch: (name) => name.toLowerCase().startsWith(`lobbylocker-${number}-`) && name.toLowerCase().endsWith('-x86_64.pkg.tar.zst'),
        appimage: (name) => name.toLowerCase() === `lobbylocker-${number}-x86_64.appimage`
      };
      const resolved = {};
      for (const [key, matches] of Object.entries(names)) {
        const asset = release.assets.find((item) => item.state === 'uploaded' && typeof item.name === 'string' && matches(item.name));
        if (!asset || typeof asset.browser_download_url !== 'string') continue;
        try {
          const url = new URL(asset.browser_download_url);
          const expected = `/its4lion/lobbylocker/releases/download/${release.tag_name}/${asset.name}`.toLowerCase();
          if (url.protocol === 'https:' && url.hostname === 'github.com' && decodeURIComponent(url.pathname).toLowerCase() === expected) resolved[key] = url.href;
        } catch { /* Ignore invalid asset URLs. */ }
      }
      downloads = resolved;
      version = release.tag_name;
      render();
    })
    .catch(() => { /* Keep the versioned direct-download fallback. */ });
})();

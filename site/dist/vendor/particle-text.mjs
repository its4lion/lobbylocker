const DEFAULT_OPTIONS = {
    lines: [],
    color: "#38bdf8",
    fontSize: 120,
    spread: 0.8,
    repelRadius: 80,
    gap: 3,
    friction: 0.92,
    returnSpeed: 0.06,
};
class ParticleText {
    canvas;
    ctx;
    opts;
    particles = [];
    mouseX = null;
    mouseY = null;
    rafId = null;
    constructor(canvas, opts = {}) {
        const ctx = canvas.getContext("2d");
        if (!ctx)
            throw new Error("ParticleText: failed to get 2D rendering context");
        this.canvas = canvas;
        this.ctx = ctx;
        const { text, ...rest } = opts;
        this.opts = { ...DEFAULT_OPTIONS, ...rest, lines: toLines(text) };
    }
    mount() {
        this.initParticles();
        this.addEventListeners();
        this.animate();
        return this;
    }
    destroy() {
        if (this.rafId !== null) {
            cancelAnimationFrame(this.rafId);
            this.rafId = null;
        }
        this.canvas.removeEventListener("mousemove", this.onMouseMove);
        this.canvas.removeEventListener("mouseleave", this.onMouseLeave);
        this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
        this.particles = [];
        return this;
    }
    update(opts) {
        const { text, fontSize, ...rest } = opts;
        Object.assign(this.opts, rest);
        if (text !== undefined || fontSize !== undefined) {
            if (fontSize !== undefined)
                this.opts.fontSize = fontSize;
            if (text !== undefined)
                this.opts.lines = toLines(text);
            return this.destroy().mount();
        }
        return this;
    }
    initParticles() {
        const { lines, fontSize, gap } = this.opts;
        const { width, height } = this.canvas;
        this.ctx.clearRect(0, 0, width, height);
        this.ctx.fillStyle = "#000";
        this.ctx.textAlign = "center";
        this.ctx.textBaseline = "top";
        this.ctx.font = `bold ${fontSize}px Arial`;
        const lineHeight = fontSize * 1.4;
        const totalHeight = lines.length * lineHeight;
        let y = (height - totalHeight) / 2;
        for (let i = 0; i < lines.length; i++) {
            this.ctx.fillText(lines[i], width / 2, y);
            y += lineHeight;
        }
        const { data } = this.ctx.getImageData(0, 0, width, height);
        this.ctx.clearRect(0, 0, width, height);
        this.particles = [];
        for (let row = 0; row < height; row += gap) {
            for (let col = 0; col < width; col += gap) {
                const alphaIndex = (row * width + col) * 4 + 3;
                if (data[alphaIndex] > 128) {
                    this.particles.push({
                        x: col,
                        y: row,
                        baseX: col,
                        baseY: row,
                        vx: 0,
                        vy: 0,
                    });
                }
            }
        }
    }
    animate = () => {
        this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
        const { color, spread, repelRadius, friction, returnSpeed } = this.opts;
        this.ctx.fillStyle = color;
        for (let i = 0; i < this.particles.length; i++) {
            const p = this.particles[i];
            if (this.mouseX !== null && this.mouseY !== null) {
                const dx = p.x - this.mouseX;
                const dy = p.y - this.mouseY;
                const dist = Math.sqrt(dx * dx + dy * dy);
                if (dist < repelRadius) {
                    const force = (repelRadius - dist) / repelRadius;
                    const angle = Math.atan2(dy, dx);
                    p.vx += Math.cos(angle) * force * spread;
                    p.vy += Math.sin(angle) * force * spread;
                }
            }
            p.vx *= friction;
            p.vy *= friction;
            p.x += p.vx + (p.baseX - p.x) * returnSpeed;
            p.y += p.vy + (p.baseY - p.y) * returnSpeed;
            this.ctx.fillRect(p.x, p.y, 2, 2);
        }
        this.rafId = requestAnimationFrame(this.animate);
    };
    onMouseMove = (e) => {
        const rect = this.canvas.getBoundingClientRect();
        this.mouseX = e.clientX - rect.left;
        this.mouseY = e.clientY - rect.top;
    };
    onMouseLeave = () => {
        this.mouseX = null;
        this.mouseY = null;
    };
    addEventListeners() {
        this.canvas.addEventListener("mousemove", this.onMouseMove);
        this.canvas.addEventListener("mouseleave", this.onMouseLeave);
    }
}
function toLines(text) {
    if (!text) {
        return DEFAULT_OPTIONS.lines;
    }
    return Array.isArray(text) ? text : [text];
}

export { ParticleText };

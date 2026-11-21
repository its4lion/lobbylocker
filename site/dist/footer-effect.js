import { ParticleText } from './vendor/particle-text.mjs';

const wrapper = document.querySelector('.footer-dotmatrix');
const canvas = document.querySelector('#footer-particles');
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
let effect = null;
let visible = true;
let resizeTimer;

function stop() {
  if (effect) effect.destroy();
  effect = null;
  wrapper.classList.remove('is-active');
}

function start() {
  stop();
  if (reducedMotion.matches || document.hidden || !visible) return;
  const width = Math.round(canvas.clientWidth);
  const height = Math.round(canvas.clientHeight);
  if (!width || !height) return;
  canvas.width = width;
  canvas.height = height;
  try {
    effect = new ParticleText(canvas, {
      text: 'LobbyLocker',
      color: '#eeeeee',
      fontSize: Math.min(240, width * .14),
      gap: 4,
      spread: 0.1,
      repelRadius: 70
    }).mount();
    wrapper.classList.add('is-active');
  } catch {
    stop();
  }
}

window.addEventListener('resize', () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(start, 150);
});
document.addEventListener('visibilitychange', start);
reducedMotion.addEventListener('change', start);
if ('IntersectionObserver' in window) {
  const observer = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    start();
  });
  observer.observe(wrapper);
}
start();

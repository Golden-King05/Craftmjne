// Keyboard / mouse / pointer-lock input manager.
// Systems poll state (isDown / justPressed / consumeMouseDelta / takeWheel).

export class Input {
  constructor(target = document.body) {
    this.target = target;
    this.keys = new Set();
    this.justKeys = new Set();
    this.buttons = new Set();
    this.justButtons = new Set();
    this.mouseDX = 0;
    this.mouseDY = 0;
    this.wheel = 0;
    this.locked = false;

    window.addEventListener('keydown', (e) => {
      if (e.repeat) return;
      this.keys.add(e.code);
      this.justKeys.add(e.code);
      if (e.code === 'F3' || e.code === 'Space' || e.code === 'Tab') e.preventDefault();
    });
    window.addEventListener('keyup', (e) => this.keys.delete(e.code));
    window.addEventListener('blur', () => this.keys.clear());

    document.addEventListener('mousemove', (e) => {
      if (!this.locked) return;
      this.mouseDX += e.movementX;
      this.mouseDY += e.movementY;
    });
    document.addEventListener('mousedown', (e) => {
      if (!this.locked) return;
      this.buttons.add(e.button);
      this.justButtons.add(e.button);
      if (e.button === 1) e.preventDefault();
    });
    document.addEventListener('mouseup', (e) => this.buttons.delete(e.button));
    document.addEventListener('contextmenu', (e) => e.preventDefault());
    document.addEventListener('wheel', (e) => {
      if (this.locked) this.wheel += Math.sign(e.deltaY);
    }, { passive: true });

    document.addEventListener('pointerlockchange', () => {
      this.locked = document.pointerLockElement != null;
      if (!this.locked) {
        this.keys.clear();
        this.buttons.clear();
      }
    });
  }

  lock() {
    const el = this.target.querySelector('canvas') ?? this.target;
    el.requestPointerLock?.()?.catch?.(() => {});
  }

  isDown(code) {
    return this.keys.has(code);
  }

  justPressed(code) {
    return this.justKeys.has(code);
  }

  buttonDown(button) {
    return this.buttons.has(button);
  }

  buttonJustPressed(button) {
    return this.justButtons.has(button);
  }

  consumeMouseDelta() {
    const d = [this.mouseDX, this.mouseDY];
    this.mouseDX = 0;
    this.mouseDY = 0;
    return d;
  }

  takeWheel() {
    const w = this.wheel;
    this.wheel = 0;
    return w;
  }

  // Called by the engine at the end of every frame.
  endFrame() {
    this.justKeys.clear();
    this.justButtons.clear();
  }
}

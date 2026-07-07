// Minimal synchronous pub/sub used for engine-wide events.
//
// Core events emitted by the default modules:
//   'chunk:generated'  ({ chunk })
//   'chunk:meshed'     ({ chunk })
//   'chunk:unloaded'   ({ chunk })
//   'block:set'        ({ x, y, z, id, prev })
//   'player:spawned'   ({ player })
//   'tick'             (dt)

export class EventBus {
  constructor() {
    this._handlers = new Map();
  }

  on(event, fn) {
    let set = this._handlers.get(event);
    if (!set) this._handlers.set(event, (set = new Set()));
    set.add(fn);
    return () => this.off(event, fn);
  }

  off(event, fn) {
    this._handlers.get(event)?.delete(fn);
  }

  once(event, fn) {
    const off = this.on(event, (...args) => {
      off();
      fn(...args);
    });
    return off;
  }

  emit(event, ...args) {
    const set = this._handlers.get(event);
    if (!set) return;
    for (const fn of [...set]) fn(...args);
  }
}

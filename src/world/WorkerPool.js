// Promise-based worker pool with least-busy dispatch.
// Takes a worker factory so Vite's `new Worker(new URL(...))` static analysis
// stays intact at the call site.

export class WorkerPool {
  constructor(createWorker, size = Math.max(2, (navigator.hardwareConcurrency || 4) - 1)) {
    this.workers = [];
    this.pending = new Map();
    this.nextId = 1;
    for (let i = 0; i < size; i++) {
      const worker = createWorker();
      worker._jobs = 0;
      worker.onmessage = (e) => {
        worker._jobs--;
        const resolve = this.pending.get(e.data.id);
        if (resolve) {
          this.pending.delete(e.data.id);
          resolve(e.data);
        }
      };
      this.workers.push(worker);
    }
  }

  get size() {
    return this.workers.length;
  }

  broadcast(msg) {
    for (const w of this.workers) w.postMessage(msg);
  }

  run(msg, transfer = []) {
    return new Promise((resolve) => {
      const id = this.nextId++;
      this.pending.set(id, resolve);
      let worker = this.workers[0];
      for (const w of this.workers) if (w._jobs < worker._jobs) worker = w;
      worker._jobs++;
      worker.postMessage({ ...msg, id }, transfer);
    });
  }

  dispose() {
    for (const w of this.workers) w.terminate();
    this.workers.length = 0;
    this.pending.clear();
  }
}

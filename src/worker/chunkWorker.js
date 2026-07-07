// Chunk worker: terrain generation and meshing off the main thread.
// The main thread never blocks on world work — see src/world/WorkerPool.js.

import { TerrainGenerator } from '../gen/TerrainGenerator.js';
import { meshChunk } from '../mesh/ChunkMesher.js';

let generator = null;
let tables = null;

self.onmessage = (e) => {
  const msg = e.data;
  switch (msg.type) {
    case 'init': {
      tables = msg.tables;
      generator = new TerrainGenerator(msg.seed, msg.ids);
      break;
    }
    case 'generate': {
      const blocks = generator.generate(msg.cx, msg.cz);
      self.postMessage({ id: msg.id, blocks }, [blocks.buffer]);
      break;
    }
    case 'mesh': {
      const result = meshChunk(msg.padded, tables);
      const transfer = [];
      for (const bucket of [result.solid, result.water]) {
        if (!bucket) continue;
        transfer.push(bucket.positions.buffer, bucket.uvs.buffer, bucket.colors.buffer, bucket.indices.buffer);
      }
      self.postMessage({ id: msg.id, solid: result.solid, water: result.water }, transfer);
      break;
    }
  }
};

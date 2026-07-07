// Voxel raycast (Amanatides & Woo DDA). Steps exactly through the grid cells
// along the ray — no float sampling, no missed corners.

/**
 * @param {number} ox,oy,oz  ray origin
 * @param {number} dx,dy,dz  ray direction (normalized)
 * @param {number} maxDist   maximum distance in blocks
 * @param {(x,y,z) => boolean} hitTest
 * @returns {{x,y,z,nx,ny,nz}|null} hit cell + face normal of entry
 */
export function raycastVoxel(ox, oy, oz, dx, dy, dz, maxDist, hitTest) {
  let x = Math.floor(ox);
  let y = Math.floor(oy);
  let z = Math.floor(oz);

  const stepX = dx > 0 ? 1 : -1;
  const stepY = dy > 0 ? 1 : -1;
  const stepZ = dz > 0 ? 1 : -1;

  const tDeltaX = dx !== 0 ? Math.abs(1 / dx) : Infinity;
  const tDeltaY = dy !== 0 ? Math.abs(1 / dy) : Infinity;
  const tDeltaZ = dz !== 0 ? Math.abs(1 / dz) : Infinity;

  let tMaxX = dx !== 0 ? ((dx > 0 ? x + 1 - ox : ox - x)) * tDeltaX : Infinity;
  let tMaxY = dy !== 0 ? ((dy > 0 ? y + 1 - oy : oy - y)) * tDeltaY : Infinity;
  let tMaxZ = dz !== 0 ? ((dz > 0 ? z + 1 - oz : oz - z)) * tDeltaZ : Infinity;

  let nx = 0, ny = 0, nz = 0;
  let t = 0;

  while (t <= maxDist) {
    if (hitTest(x, y, z)) {
      if (nx === 0 && ny === 0 && nz === 0) return null; // started inside a block
      return { x, y, z, nx, ny, nz };
    }
    if (tMaxX < tMaxY && tMaxX < tMaxZ) {
      x += stepX;
      t = tMaxX;
      tMaxX += tDeltaX;
      nx = -stepX; ny = 0; nz = 0;
    } else if (tMaxY < tMaxZ) {
      y += stepY;
      t = tMaxY;
      tMaxY += tDeltaY;
      nx = 0; ny = -stepY; nz = 0;
    } else {
      z += stepZ;
      t = tMaxZ;
      tMaxZ += tDeltaZ;
      nx = 0; ny = 0; nz = -stepZ;
    }
  }
  return null;
}

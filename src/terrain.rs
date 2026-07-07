//! Procedural terrain generator. Runs on the async compute task pool.
//! Deterministic per (seed, cx, cz) with no cross-chunk dependencies, so
//! chunks can generate in any order on any thread. Trees keep a 2-block
//! margin from chunk borders so features never spill across chunks.
//!
//! To customize generation, swap the generator constructed in
//! `world::compile_content` for your own.

use crate::blocks::{BlockId, BlockRegistry, AIR};
use crate::config::{block_index, CHUNK_SIZE, CS, H, SEA_LEVEL, WORLD_HEIGHT};
use crate::noise::{hash2, hash3, SimplexNoise};

const SNOW_LINE: i32 = 45;

struct TerrainIds {
    stone: BlockId,
    dirt: BlockId,
    grass: BlockId,
    sand: BlockId,
    gravel: BlockId,
    water: BlockId,
    log: BlockId,
    leaves: BlockId,
    bedrock: BlockId,
    snow: BlockId,
    coal: BlockId,
    iron: BlockId,
}

pub struct TerrainGenerator {
    seed: u32,
    ids: TerrainIds,
    terrain: SimplexNoise,
    mountain: SimplexNoise,
    cave_a: SimplexNoise,
    cave_b: SimplexNoise,
}

impl TerrainGenerator {
    pub fn new(seed: u32, reg: &BlockRegistry) -> Self {
        Self {
            seed,
            ids: TerrainIds {
                stone: reg.id("stone"),
                dirt: reg.id("dirt"),
                grass: reg.id("grass"),
                sand: reg.id("sand"),
                gravel: reg.id("gravel"),
                water: reg.id("water"),
                log: reg.id("log"),
                leaves: reg.id("leaves"),
                bedrock: reg.id("bedrock"),
                snow: reg.id("snow"),
                coal: reg.id("coal_ore"),
                iron: reg.id("iron_ore"),
            },
            terrain: SimplexNoise::new(seed),
            mountain: SimplexNoise::new(seed ^ 0x9e3779b9),
            cave_a: SimplexNoise::new(seed ^ 0x85ebca6b),
            cave_b: SimplexNoise::new(seed ^ 0xc2b2ae35),
        }
    }

    pub fn surface_height(&self, wx: i32, wz: i32) -> i32 {
        // Low-frequency mask blends flat plains into mountains.
        let m = self.mountain.fbm2(wx as f64 * 0.0035, wz as f64 * 0.0035, 3) * 0.5 + 0.5;
        let mountain = m * m;
        let detail = self.terrain.fbm2(wx as f64 * 0.011, wz as f64 * 0.011, 4);
        let h = 27.0 + detail * (5.0 + mountain * 24.0) + mountain * 10.0;
        (h.floor() as i32).clamp(2, WORLD_HEIGHT - 8)
    }

    pub fn generate(&self, cx: i32, cz: i32) -> Vec<BlockId> {
        let ids = &self.ids;
        let seed = self.seed;
        let mut blocks = vec![AIR; CS * CS * H];
        let mut heights = [0i32; CS * CS];
        let mut surface = [AIR; CS * CS];

        for z in 0..CS {
            for x in 0..CS {
                let wx = cx * CHUNK_SIZE + x as i32;
                let wz = cz * CHUNK_SIZE + z as i32;
                let h = self.surface_height(wx, wz);
                heights[x + CS * z] = h;

                let beach = h <= SEA_LEVEL + 1;
                let snowy = h >= SNOW_LINE;
                let top_id = if beach { ids.sand } else if snowy { ids.snow } else { ids.grass };
                let fill_id = if beach { ids.sand } else { ids.dirt };
                surface[x + CS * z] = top_id;

                let base = block_index(x, 0, z);
                blocks[base] = ids.bedrock;
                for y in 1..=h {
                    blocks[base + y as usize] = if y == h {
                        top_id
                    } else if y >= h - 3 {
                        fill_id
                    } else {
                        ids.stone
                    };
                }
                // Flood water up to sea level.
                for y in (h + 1)..=SEA_LEVEL {
                    blocks[base + y as usize] = ids.water;
                }
                // Gravel patches on the sea floor.
                if h < SEA_LEVEL && hash2(wx, wz, seed ^ 0x1234) < 0.3 {
                    blocks[base + h as usize] = ids.gravel;
                }

                // Carve "spaghetti" caves on land columns (kept away from
                // water so we don't punch holes into the sea floor).
                if h > SEA_LEVEL + 1 {
                    for y in 4..(h - 2) {
                        let a = self
                            .cave_a
                            .noise3(wx as f64 * 0.045, y as f64 * 0.075, wz as f64 * 0.045);
                        if a.abs() > 0.09 {
                            continue;
                        }
                        let b = self
                            .cave_b
                            .noise3(wx as f64 * 0.045, y as f64 * 0.075, wz as f64 * 0.045);
                        if b.abs() < 0.09 {
                            blocks[base + y as usize] = AIR;
                        }
                    }
                }

                // Ore veins.
                for y in 2..(h - 3).min(40) {
                    if blocks[base + y as usize] != ids.stone {
                        continue;
                    }
                    let r = hash3(wx, y, wz, seed ^ 0xabcd);
                    if r < 0.006 && y < 28 {
                        blocks[base + y as usize] = ids.iron;
                    } else if r < 0.018 {
                        blocks[base + y as usize] = ids.coal;
                    }
                }
            }
        }

        // Trees (second pass; margin keeps canopies inside this chunk).
        for z in 2..CS - 2 {
            for x in 2..CS - 2 {
                if surface[x + CS * z] != ids.grass {
                    continue;
                }
                let wx = cx * CHUNK_SIZE + x as i32;
                let wz = cz * CHUNK_SIZE + z as i32;
                let r = hash2(wx, wz, seed ^ 0x51f3);
                if r >= 0.012 {
                    continue;
                }

                let h = heights[x + CS * z];
                let trunk_h = 4 + ((r * 1000.0) as i32) % 3;
                if h + trunk_h + 2 >= WORLD_HEIGHT {
                    continue;
                }
                let base = block_index(x, 0, z);
                if blocks[base + h as usize] != ids.grass {
                    continue; // surface was carved away
                }

                blocks[base + h as usize] = ids.dirt;
                for t in 1..=trunk_h {
                    blocks[base + (h + t) as usize] = ids.log;
                }

                // Canopy: two wide layers, a narrow layer, and a cap.
                for ly in (trunk_h - 2)..=(trunk_h + 1) {
                    let radius: i32 = if ly <= trunk_h - 1 { 2 } else { 1 };
                    for dz in -radius..=radius {
                        for dx in -radius..=radius {
                            if dx.abs() == radius
                                && dz.abs() == radius
                                && (radius == 2 || ly == trunk_h + 1)
                            {
                                continue; // clip corners
                            }
                            let idx = block_index(
                                (x as i32 + dx) as usize,
                                (h + ly) as usize,
                                (z as i32 + dz) as usize,
                            );
                            if blocks[idx] == AIR {
                                blocks[idx] = ids.leaves;
                            }
                        }
                    }
                }
                blocks[base + (h + trunk_h + 1) as usize] = ids.leaves;
            }
        }

        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::BlockRegistry;

    #[test]
    fn generation_is_deterministic_and_sane() {
        let reg = BlockRegistry::with_defaults();
        let gen = TerrainGenerator::new(7, &reg);
        let a = gen.generate(3, -2);
        let b = gen.generate(3, -2);
        assert_eq!(a, b);
        assert_eq!(a.len(), CS * CS * H);

        let bedrock = reg.id("bedrock");
        let stone = reg.id("stone");
        for z in 0..CS {
            for x in 0..CS {
                assert_eq!(a[block_index(x, 0, z)], bedrock);
                // top of the world is air
                assert_eq!(a[block_index(x, H - 1, z)], AIR);
            }
        }
        assert!(a.iter().any(|&b| b == stone));
    }

    #[test]
    fn different_seeds_differ() {
        let reg = BlockRegistry::with_defaults();
        let a = TerrainGenerator::new(1, &reg).generate(0, 0);
        let b = TerrainGenerator::new(2, &reg).generate(0, 0);
        assert_ne!(a, b);
    }
}

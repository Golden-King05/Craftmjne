// Three.js renderer for chunk meshes.
//
// Rendering strategy (for speed):
//  - All lighting is pre-baked into vertex colors by the mesher, so materials
//    are unlit MeshBasicMaterial — no lights, no normals, no shadow passes.
//  - One shared material for all solid geometry (alphaTest handles leaf/glass
//    cutouts) and one for water: two pipeline states total.
//  - One mesh per chunk pass; three.js frustum-culls per chunk via precomputed
//    bounding spheres (skipping three's per-geometry bounds computation).
//  - Distance fog hides the streaming edge of the world.

import * as THREE from 'three';
import { CHUNK_SIZE as CS, WORLD_HEIGHT as H } from '../config.js';

const SKY_COLOR = 0x87b9e6;

export class Renderer {
  constructor(container, atlas, config) {
    this.renderer = new THREE.WebGLRenderer({ antialias: false, powerPreference: 'high-performance' });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.setSize(window.innerWidth, window.innerHeight);
    container.appendChild(this.renderer.domElement);

    const viewDist = config.renderDistance * CS;
    this.scene = new THREE.Scene();
    this.scene.background = new THREE.Color(SKY_COLOR);
    this.scene.fog = new THREE.Fog(SKY_COLOR, viewDist * 0.55, viewDist * 0.95);

    this.camera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.1, viewDist * 2);
    this.camera.rotation.order = 'YXZ';
    this.camera.position.set(8, H, 8);

    this.solidMaterial = new THREE.MeshBasicMaterial({
      map: atlas.texture,
      vertexColors: true,
      alphaTest: 0.5, // leaf / glass cutouts
    });
    this.waterMaterial = new THREE.MeshBasicMaterial({
      map: atlas.texture,
      vertexColors: true,
      transparent: true,
      opacity: 0.72,
      depthWrite: false,
      side: THREE.DoubleSide,
    });

    this.chunkMeshes = new Map(); // key -> { solid?, water? }

    // Precomputed chunk bounds (identical for every chunk, in local coords).
    this.chunkSphere = new THREE.Sphere(
      new THREE.Vector3(CS / 2, H / 2, CS / 2),
      Math.sqrt(2 * (CS / 2) ** 2 + (H / 2) ** 2) + 1,
    );

    // Block highlight wireframe.
    this.highlight = new THREE.LineSegments(
      new THREE.EdgesGeometry(new THREE.BoxGeometry(1.002, 1.002, 1.002)),
      new THREE.LineBasicMaterial({ color: 0x111111, transparent: true, opacity: 0.7 }),
    );
    this.highlight.visible = false;
    this.scene.add(this.highlight);

    window.addEventListener('resize', () => {
      this.camera.aspect = window.innerWidth / window.innerHeight;
      this.camera.updateProjectionMatrix();
      this.renderer.setSize(window.innerWidth, window.innerHeight);
    });
  }

  makeGeometry(data) {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute('position', new THREE.BufferAttribute(data.positions, 3));
    geometry.setAttribute('uv', new THREE.BufferAttribute(data.uvs, 2));
    geometry.setAttribute('color', new THREE.BufferAttribute(data.colors, 3, true));
    geometry.setIndex(new THREE.BufferAttribute(data.indices, 1));
    geometry.boundingSphere = this.chunkSphere.clone();
    return geometry;
  }

  setChunkMesh(key, cx, cz, result) {
    this.removeChunkMesh(key);
    const entry = {};
    for (const [pass, data, material] of [
      ['solid', result.solid, this.solidMaterial],
      ['water', result.water, this.waterMaterial],
    ]) {
      if (!data) continue;
      const mesh = new THREE.Mesh(this.makeGeometry(data), material);
      mesh.position.set(cx * CS, 0, cz * CS);
      mesh.matrixAutoUpdate = false;
      mesh.updateMatrix();
      if (pass === 'water') mesh.renderOrder = 1;
      this.scene.add(mesh);
      entry[pass] = mesh;
    }
    this.chunkMeshes.set(key, entry);
  }

  removeChunkMesh(key) {
    const entry = this.chunkMeshes.get(key);
    if (!entry) return;
    for (const mesh of Object.values(entry)) {
      this.scene.remove(mesh);
      mesh.geometry.dispose();
    }
    this.chunkMeshes.delete(key);
  }

  setHighlight(hit) {
    if (hit) {
      this.highlight.position.set(hit.x + 0.5, hit.y + 0.5, hit.z + 0.5);
      this.highlight.visible = true;
    } else {
      this.highlight.visible = false;
    }
  }

  render() {
    this.renderer.render(this.scene, this.camera);
  }

  get info() {
    return this.renderer.info;
  }
}

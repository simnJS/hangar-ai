import {
  MeshStandardMaterial,
  NoColorSpace,
  SRGBColorSpace,
  type Material,
  type MeshLambertMaterial,
  type MeshPhongMaterial,
  type Object3D,
  type Texture,
} from "three";

/**
 * Every material the viewer draws, brought to the same footing.
 *
 * OBJ, FBX and the plain geometry formats come back with Phong or Lambert
 * materials, which ignore the studio environment that lights everything
 * else: next to a glTF they would look flat and dark, and the user would be
 * judging the viewer instead of the asset. They become standard (PBR)
 * materials with the same maps and colours, and a roughness that keeps the
 * highlight the shininess described. Unlit (basic) materials stay unlit:
 * that is a choice the artist made.
 */

type Legacy = MeshPhongMaterial | MeshLambertMaterial;

const isLegacy = (material: Material): material is Legacy =>
  (material as MeshPhongMaterial).isMeshPhongMaterial === true ||
  (material as MeshLambertMaterial).isMeshLambertMaterial === true;

/** Colour maps hold sRGB colours; a loader that forgot to say so gets corrected. */
function asColor(texture: Texture | null) {
  if (texture && texture.colorSpace === NoColorSpace) {
    texture.colorSpace = SRGBColorSpace;
    texture.needsUpdate = true;
  }
  return texture;
}

/**
 * Blinn-Phong's exponent to a GGX roughness, by matching the lobes' widths:
 * a shininess of 30, Phong's default, comes out at 0.5.
 */
export const roughnessFromShininess = (shininess: number) =>
  Math.min(1, Math.max(0.04, (2 / (Math.max(shininess, 0) + 2)) ** 0.25));

export function toStandard(material: Legacy): MeshStandardMaterial {
  const phong = (material as MeshPhongMaterial).isMeshPhongMaterial ? (material as MeshPhongMaterial) : null;
  // Set field by field rather than through the constructor, which warns
  // about every slot a Lambert material does not have.
  const standard = new MeshStandardMaterial();
  standard.name = material.name;
  standard.color.copy(material.color);
  standard.map = asColor(material.map);
  standard.emissive.copy(material.emissive);
  standard.emissiveMap = asColor(material.emissiveMap);
  standard.emissiveIntensity = material.emissiveIntensity;
  standard.alphaMap = material.alphaMap;
  standard.aoMap = material.aoMap;
  standard.aoMapIntensity = material.aoMapIntensity;
  standard.lightMap = material.lightMap;
  standard.lightMapIntensity = material.lightMapIntensity;
  if (phong) {
    standard.normalMap = phong.normalMap;
    standard.normalScale.copy(phong.normalScale);
    standard.bumpMap = phong.bumpMap;
    standard.bumpScale = phong.bumpScale;
    standard.displacementMap = phong.displacementMap;
    standard.displacementScale = phong.displacementScale;
    standard.displacementBias = phong.displacementBias;
    standard.flatShading = phong.flatShading;
  }
  standard.roughness = phong ? roughnessFromShininess(phong.shininess) : 1;
  standard.metalness = 0;
  standard.vertexColors = material.vertexColors;
  standard.transparent = material.transparent;
  standard.opacity = material.opacity;
  standard.alphaTest = material.alphaTest;
  standard.side = material.side;
  standard.depthWrite = material.depthWrite;
  standard.visible = material.visible;
  standard.userData = { ...material.userData };
  return standard;
}

/**
 * Converts the legacy materials of a model in place. A material shared by
 * several meshes is converted once and stays shared, so the counts do not
 * change.
 */
export function standardizeMaterials(root: Object3D) {
  const converted = new Map<Material, Material>();
  const convert = (material: Material) => {
    if (!isLegacy(material)) {
      const textured = material as MeshStandardMaterial;
      if ("map" in textured) asColor(textured.map);
      if ("emissiveMap" in textured) asColor(textured.emissiveMap);
      return material;
    }
    let next = converted.get(material);
    if (!next) {
      next = toStandard(material);
      converted.set(material, next);
    }
    return next;
  };

  root.traverse((object) => {
    const node = object as Object3D & { material?: Material | Material[] };
    if (!node.material) return;
    node.material = Array.isArray(node.material) ? node.material.map(convert) : convert(node.material);
  });
  // The textures moved to the new materials; only the old shells go.
  for (const old of converted.keys()) old.dispose();
}

/** Every texture a material holds, whatever slot it sits in. */
export function texturesOf(material: Material): Texture[] {
  const found: Texture[] = [];
  for (const value of Object.values(material)) {
    if (value && typeof value === "object" && (value as Texture).isTexture) found.push(value as Texture);
  }
  const uniforms = (material as Material & { uniforms?: Record<string, { value: unknown }> }).uniforms;
  if (uniforms) {
    for (const { value } of Object.values(uniforms)) {
      if (value && typeof value === "object" && (value as Texture).isTexture) found.push(value as Texture);
    }
  }
  return found;
}

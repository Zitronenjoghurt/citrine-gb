use super::{RomError, slice, u16_at, u32_at};

pub type Matrix = [[f32; 4]; 4];

pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Load(u8),
    Store(u8),
    Object(u8),
    Material(u8),
    Draw(u8),
    ScaleUp,
    ScaleDown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub diffuse_ambient: u32,
    pub specular_emission: u32,
    pub polygon_attr: u32,
    pub teximage_param: u32,
    pub texture: Option<String>,
    pub palette: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Model<'a> {
    pub name: String,
    pub up_scale: f32,
    pub down_scale: f32,
    pub bounds: [[f32; 3]; 2],
    pub objects: Vec<Matrix>,
    pub steps: Vec<Step>,
    pub materials: Vec<Material>,
    pub pieces: Vec<&'a [u8]>,
}

pub fn models(file: &[u8]) -> Result<Vec<Model<'_>>, RomError> {
    let sections = usize::from(u16_at(file, 0x0E)?);
    for i in 0..sections {
        let at = u32_at(file, 0x10 + i * 4)? as usize;
        if file.get(at..at + 4) == Some(b"MDL0") {
            let mdl0 = &file[at..];
            return dict(mdl0, 8, 4)?
                .into_iter()
                .map(|(name, entry)| model(mdl0, word(&entry) as usize, name))
                .collect();
        }
    }
    Err(RomError::MissingSection("MDL0"))
}

fn word(entry: &[u8]) -> u32 {
    u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]])
}

fn dict(bytes: &[u8], at: usize, unit: usize) -> Result<Vec<(String, Vec<u8>)>, RomError> {
    let count = usize::from(*bytes.get(at + 1).ok_or(RomError::Truncated(at + 2))?);
    let data = at + 0x10 + 4 * count;
    let names = data + count * unit;
    (0..count)
        .map(|i| {
            let entry = slice(bytes, data + i * unit, data + (i + 1) * unit)?.to_vec();
            let raw = slice(bytes, names + i * 16, names + (i + 1) * 16)?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            Ok((String::from_utf8_lossy(&raw[..end]).into_owned(), entry))
        })
        .collect()
}

fn fixed32(bytes: &[u8], at: usize) -> Result<f32, RomError> {
    Ok(u32_at(bytes, at)? as i32 as f32 / 4096.0)
}

fn fixed16(bytes: &[u8], at: usize) -> Result<f32, RomError> {
    Ok(u16_at(bytes, at)? as i16 as f32 / 4096.0)
}

fn model<'a>(mdl0: &'a [u8], at: usize, name: String) -> Result<Model<'a>, RomError> {
    let base = mdl0.get(at..).ok_or(RomError::Truncated(at))?;
    let steps_at = u32_at(base, 0x04)? as usize;
    let materials_at = u32_at(base, 0x08)? as usize;
    let pieces_at = u32_at(base, 0x0C)? as usize;
    let box_scale = match fixed32(base, 0x38)? {
        0.0 => 1.0,
        scale => scale,
    };
    let (mut low, mut high) = ([0.0; 3], [0.0; 3]);
    for (i, (min, max)) in low.iter_mut().zip(&mut high).enumerate() {
        let start = fixed16(base, 0x2C + i * 2)?;
        let size = fixed16(base, 0x32 + i * 2)?;
        *min = start * box_scale;
        *max = (start + size) * box_scale;
    }
    let bounds = [low, high];
    let objects = dict(base, 0x40, 4)?
        .into_iter()
        .map(|(_, entry)| object(base, 0x40 + word(&entry) as usize))
        .collect::<Result<_, _>>()?;
    let pieces = dict(base, pieces_at, 4)?
        .into_iter()
        .map(|(_, entry)| {
            let start = pieces_at + word(&entry) as usize;
            let offset = u32_at(base, start + 0x08)? as usize;
            let len = u32_at(base, start + 0x0C)? as usize;
            slice(base, start + offset, start + offset + len)
        })
        .collect::<Result<_, _>>()?;
    Ok(Model {
        name,
        up_scale: fixed32(base, 0x1C)?,
        down_scale: fixed32(base, 0x20)?,
        bounds,
        objects,
        steps: steps(base.get(steps_at..).unwrap_or(&[]))?,
        materials: materials(base, materials_at)?,
        pieces,
    })
}

fn object(base: &[u8], at: usize) -> Result<Matrix, RomError> {
    let flags = u16_at(base, at)?;
    let mut cursor = at + 4;
    let mut translate = [0.0; 3];
    if flags & 1 == 0 {
        for (i, value) in translate.iter_mut().enumerate() {
            *value = fixed32(base, cursor + i * 4)?;
        }
        cursor += 12;
    }
    let mut rotate = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    if flags & 8 != 0 {
        let a = fixed16(base, cursor)?;
        let b = fixed16(base, cursor + 2)?;
        rotate = pivot((flags >> 4) & 15, (flags >> 8) & 15, a, b);
        cursor += 4;
    } else if flags & 2 == 0 {
        let mut values = [fixed16(base, at + 2)?; 9];
        for (i, value) in values.iter_mut().enumerate().skip(1) {
            *value = fixed16(base, cursor + (i - 1) * 2)?;
        }
        for (column, values) in values.chunks(3).enumerate() {
            for (row, &value) in values.iter().enumerate() {
                rotate[row][column] = value;
            }
        }
        cursor += 16;
    }
    let mut scale = [1.0; 3];
    if flags & 4 == 0 {
        for (i, value) in scale.iter_mut().enumerate() {
            *value = fixed32(base, cursor + i * 4)?;
        }
    }
    let mut matrix = IDENTITY;
    for row in 0..3 {
        for column in 0..3 {
            matrix[row][column] = rotate[row][column] * scale[column];
        }
        matrix[row][3] = translate[row];
    }
    Ok(matrix)
}

fn pivot(select: u16, neg: u16, a: f32, b: f32) -> [[f32; 3]; 3] {
    let mut m = [[0.0; 3]; 3];
    if select >= 9 {
        m[0][0] = -a;
        return m;
    }
    let sign = |bit: u16, value: f32| if neg & bit != 0 { -value } else { value };
    let (column, row) = (usize::from(select / 3), usize::from(select % 3));
    m[row][column] = sign(1, 1.0);
    let rows: Vec<usize> = (0..3).filter(|&r| r != row).collect();
    let columns: Vec<usize> = (0..3).filter(|&c| c != column).collect();
    m[rows[0]][columns[0]] = a;
    m[rows[1]][columns[0]] = b;
    m[rows[0]][columns[1]] = sign(2, b);
    m[rows[1]][columns[1]] = sign(4, a);
    m
}

fn steps(bytes: &[u8]) -> Result<Vec<Step>, RomError> {
    let mut steps = Vec::new();
    let mut at = 0;
    let arg = |i: usize| bytes.get(i).copied().ok_or(RomError::Truncated(i + 1));
    loop {
        let op = arg(at)?;
        let len = match op {
            0x00 | 0x0B | 0x2B | 0x40 | 0x80 => 1,
            0x01 => return Ok(steps),
            0x03 | 0x04 | 0x24 | 0x44 | 0x05 | 0x07 | 0x08 => 2,
            0x02 | 0x47 | 0x0C | 0x0D => 3,
            0x06 => 4,
            0x26 | 0x46 => 5,
            0x66 => 6,
            0x09 => 3 + 3 * usize::from(arg(at + 2)?),
            _ => return Err(RomError::BadRenderStep(at)),
        };
        match op {
            0x03 => steps.push(Step::Load(arg(at + 1)?)),
            0x04 | 0x24 | 0x44 => steps.push(Step::Material(arg(at + 1)?)),
            0x05 => steps.push(Step::Draw(arg(at + 1)?)),
            0x06 => steps.push(Step::Object(arg(at + 1)?)),
            0x26 => {
                steps.push(Step::Object(arg(at + 1)?));
                steps.push(Step::Store(arg(at + 4)?));
            }
            0x46 => {
                steps.push(Step::Load(arg(at + 4)?));
                steps.push(Step::Object(arg(at + 1)?));
            }
            0x66 => {
                steps.push(Step::Load(arg(at + 5)?));
                steps.push(Step::Object(arg(at + 1)?));
                steps.push(Step::Store(arg(at + 4)?));
            }
            0x0B => steps.push(Step::ScaleUp),
            0x2B => steps.push(Step::ScaleDown),
            _ => {}
        }
        at += len;
    }
}

fn materials(base: &[u8], at: usize) -> Result<Vec<Material>, RomError> {
    let texture_pairs = at + usize::from(u16_at(base, at)?);
    let palette_pairs = at + usize::from(u16_at(base, at + 2)?);
    let mut materials: Vec<Material> = dict(base, at + 4, 4)?
        .into_iter()
        .map(|(name, entry)| {
            let start = at + word(&entry) as usize;
            Ok(Material {
                name,
                diffuse_ambient: u32_at(base, start + 0x04)?,
                specular_emission: u32_at(base, start + 0x08)?,
                polygon_attr: u32_at(base, start + 0x0C)?,
                teximage_param: u32_at(base, start + 0x14)? & u32_at(base, start + 0x10)?,
                texture: None,
                palette: None,
            })
        })
        .collect::<Result<_, RomError>>()?;
    for (pairs, texture) in [(texture_pairs, true), (palette_pairs, false)] {
        for (name, entry) in dict(base, pairs, 4)? {
            let ids = at + usize::from(u16::from_le_bytes([entry[0], entry[1]]));
            for &id in slice(base, ids, ids + usize::from(entry[2]))? {
                if let Some(material) = materials.get_mut(usize::from(id)) {
                    let slot = if texture {
                        &mut material.texture
                    } else {
                        &mut material.palette
                    };
                    *slot = Some(name.clone());
                }
            }
        }
    }
    Ok(materials)
}

#[cfg(any(test, feature = "fixtures"))]
fn fixture_dict(entries: &[(&str, u32)]) -> Vec<u8> {
    let count = entries.len();
    let mut out = vec![0, count as u8, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0];
    out.extend(std::iter::repeat_n(0, 4 * count));
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&((4 + count * 4) as u16).to_le_bytes());
    for (_, value) in entries {
        out.extend_from_slice(&value.to_le_bytes());
    }
    for (name, _) in entries {
        let mut raw = name.as_bytes().to_vec();
        raw.resize(16, 0);
        out.extend(raw);
    }
    out
}

#[cfg(any(test, feature = "fixtures"))]
fn fixture_dict_len(count: usize) -> usize {
    0x10 + 4 * count + 4 * count + 16 * count
}

#[cfg(any(test, feature = "fixtures"))]
pub fn build_triangle(polygon_attr: u32) -> Vec<u8> {
    let vertex = |x: f32, y: f32, z: f32| {
        let pack = |v: f32| u32::from((v * 4096.0).round() as i16 as u16);
        [pack(x) | pack(y) << 16, pack(z)]
    };
    let mut gx = vec![u32::from_le_bytes([0x40, 0x20, 0x23, 0x23]), 0, 0x7FFF];
    gx.extend(vertex(-1.0, -1.0, 0.0));
    gx.extend(vertex(1.0, -1.0, 0.0));
    gx.push(u32::from_le_bytes([0x23, 0x41, 0x00, 0x00]));
    gx.extend(vertex(0.0, 1.0, 0.0));
    let gx: Vec<u8> = gx.iter().flat_map(|w| w.to_le_bytes()).collect();

    let objects_len = fixture_dict_len(1) + 4;
    let steps_at = 0x40 + objects_len;
    let steps = [0x06, 0, 0, 0, 0x04, 0, 0x05, 0, 0x01, 0, 0, 0];
    let materials_at = steps_at + steps.len();
    let material_dict_len = fixture_dict_len(1);
    let empty_pairs = fixture_dict(&[]);
    let texture_pairs = 4 + material_dict_len;
    let palette_pairs = texture_pairs + empty_pairs.len();
    let material_at = palette_pairs + empty_pairs.len();
    let mut materials = Vec::new();
    materials.extend_from_slice(&(texture_pairs as u16).to_le_bytes());
    materials.extend_from_slice(&(palette_pairs as u16).to_le_bytes());
    materials.extend(fixture_dict(&[("mat", material_at as u32)]));
    materials.extend(&empty_pairs);
    materials.extend(&empty_pairs);
    let mut material = vec![0u8; 0x2C];
    material[0x04..0x08].copy_from_slice(&0x7FFFu32.to_le_bytes());
    material[0x0C..0x10].copy_from_slice(&polygon_attr.to_le_bytes());
    material[0x10..0x14].copy_from_slice(&u32::MAX.to_le_bytes());
    materials.extend(material);
    let pieces_at = materials_at + materials.len();
    let piece_at = fixture_dict_len(1);
    let mut pieces = fixture_dict(&[("shape", piece_at as u32)]);
    let mut piece = vec![0u8; 0x10];
    piece[0x08..0x0C].copy_from_slice(&0x10u32.to_le_bytes());
    piece[0x0C..0x10].copy_from_slice(&(gx.len() as u32).to_le_bytes());
    pieces.extend(piece);
    pieces.extend(gx);

    let mut model = vec![0u8; 0x40];
    let total = pieces_at + pieces.len();
    model[0x00..0x04].copy_from_slice(&(total as u32).to_le_bytes());
    model[0x04..0x08].copy_from_slice(&(steps_at as u32).to_le_bytes());
    model[0x08..0x0C].copy_from_slice(&(materials_at as u32).to_le_bytes());
    model[0x0C..0x10].copy_from_slice(&(pieces_at as u32).to_le_bytes());
    model[0x17] = 1;
    model[0x1C..0x20].copy_from_slice(&4096u32.to_le_bytes());
    model[0x20..0x24].copy_from_slice(&4096u32.to_le_bytes());
    for (i, value) in [-1i16, -1, 0, 2, 2, 0].iter().enumerate() {
        model[0x2C + i * 2..0x2E + i * 2].copy_from_slice(&(value * 4096).to_le_bytes());
    }
    model[0x38..0x3C].copy_from_slice(&4096u32.to_le_bytes());
    model.extend(fixture_dict(&[("node", fixture_dict_len(1) as u32)]));
    model.extend_from_slice(&[0x07, 0x00, 0x00, 0x10]);
    model.extend_from_slice(&steps);
    model.extend(materials);
    model.extend(pieces);

    let model_at = 8 + fixture_dict_len(1);
    let mut mdl0 = b"MDL0".to_vec();
    mdl0.extend_from_slice(&((model_at + model.len()) as u32).to_le_bytes());
    mdl0.extend(fixture_dict(&[("triangle", model_at as u32)]));
    mdl0.extend(model);
    let mut file = b"BMD0\xFF\xFE\x02\x00".to_vec();
    file.extend_from_slice(&((0x14 + mdl0.len()) as u32).to_le_bytes());
    file.extend_from_slice(&[0x10, 0, 1, 0]);
    file.extend_from_slice(&0x14u32.to_le_bytes());
    file.extend(mdl0);
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_the_render_bytecode() {
        let bytes = [
            0x06, 0, 0, 0, 0x26, 1, 0, 0, 3, 0x04, 0, 0x05, 0, 0x0B, 0x66, 2, 1, 0, 4, 3, 0x09, 5,
            1, 0, 0, 255, 0x01,
        ];
        assert_eq!(
            steps(&bytes).unwrap(),
            [
                Step::Object(0),
                Step::Object(1),
                Step::Store(3),
                Step::Material(0),
                Step::Draw(0),
                Step::ScaleUp,
                Step::Load(3),
                Step::Object(2),
                Step::Store(4),
            ]
        );
    }

    #[test]
    fn an_unknown_step_is_an_error() {
        assert_eq!(steps(&[0x00, 0x77]), Err(RomError::BadRenderStep(1)));
        assert!(steps(&[0x05]).is_err());
    }

    #[test]
    fn pivot_rotations_place_the_pivot_and_signs() {
        assert_eq!(
            pivot(0, 0, 0.5, 0.25),
            [[1.0, 0.0, 0.0], [0.0, 0.5, 0.25], [0.0, 0.25, 0.5]]
        );
        assert_eq!(
            pivot(4, 0b111, 1.0, 2.0),
            [[1.0, 0.0, -2.0], [0.0, -1.0, 0.0], [2.0, 0.0, -1.0]]
        );
    }

    #[test]
    fn objects_compose_translate_rotate_and_scale() {
        let mut bytes = vec![0u8; 4];
        bytes[0] = 0b0010;
        for value in [1i32, 2, 3] {
            bytes.extend_from_slice(&(value * 4096).to_le_bytes());
        }
        for value in [2i32, 2, 2] {
            bytes.extend_from_slice(&(value * 4096).to_le_bytes());
        }
        let matrix = object(&bytes, 0).unwrap();
        assert_eq!(matrix[0], [2.0, 0.0, 0.0, 1.0]);
        assert_eq!(matrix[2], [0.0, 0.0, 2.0, 3.0]);
    }

    #[test]
    fn a_file_without_models_says_so() {
        let file = b"BTX0\xFF\xFE\x01\x00\x14\x00\x00\x00\x10\x00\x00\x00".to_vec();
        assert_eq!(models(&file).unwrap_err(), RomError::MissingSection("MDL0"));
    }

    #[test]
    fn the_triangle_fixture_reads_back() {
        let file = build_triangle(31 << 16 | 0xC0);
        let list = models(&file).unwrap();
        let model = &list[0];
        assert_eq!(model.name, "triangle");
        assert_eq!(model.objects, [IDENTITY]);
        assert_eq!(model.up_scale, 1.0);
        assert_eq!(model.bounds, [[-1.0, -1.0, 0.0], [1.0, 1.0, 0.0]]);
        assert_eq!(
            model.steps,
            [Step::Object(0), Step::Material(0), Step::Draw(0)]
        );
        assert_eq!(model.materials[0].name, "mat");
        assert_eq!(model.materials[0].polygon_attr, 31 << 16 | 0xC0);
        assert_eq!(model.materials[0].texture, None);
        assert_eq!(model.pieces[0].len(), 40);
    }
}

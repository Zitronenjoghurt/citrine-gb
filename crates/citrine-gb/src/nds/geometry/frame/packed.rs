use super::{MAX_POLYGON_VERTICES, Polygon, Vertex};
use serde::de::{DeserializeOwned, Error, SeqAccess, Visitor};
use serde::{Deserializer, Serializer};
use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;

pub(super) trait Packed: Sized + DeserializeOwned {
    const SIZE: usize;

    fn pack(&self, out: &mut Vec<u8>);

    fn unpack(from: &mut Reader<'_>) -> Self;
}

pub(super) struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let (head, rest) = self.bytes.split_at(N);
        self.bytes = rest;
        head.try_into().unwrap_or([0; N])
    }

    fn u8(&mut self) -> u8 {
        self.take::<1>()[0]
    }

    fn bool(&mut self) -> bool {
        self.u8() != 0
    }

    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }

    fn i16(&mut self) -> i16 {
        i16::from_le_bytes(self.take())
    }

    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }

    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take())
    }
}

impl Packed for Vertex {
    const SIZE: usize = 30;

    fn pack(&self, out: &mut Vec<u8>) {
        for value in self.position {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in self.screen {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.subpixel);
        out.extend_from_slice(&self.color);
        for value in self.texcoord {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.push(u8::from(self.clipped));
    }

    fn unpack(from: &mut Reader<'_>) -> Self {
        Self {
            position: std::array::from_fn(|_| from.i32()),
            screen: std::array::from_fn(|_| from.u16()),
            subpixel: std::array::from_fn(|_| from.u8()),
            color: std::array::from_fn(|_| from.u8()),
            texcoord: std::array::from_fn(|_| from.i16()),
            clipped: from.bool(),
        }
    }
}

impl Packed for Polygon {
    const SIZE: usize = 2 * MAX_POLYGON_VERTICES + 1 + 8 * MAX_POLYGON_VERTICES + 16;

    fn pack(&self, out: &mut Vec<u8>) {
        for value in self.vertices {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.push(self.count);
        for value in self.depth.iter().chain(&self.w) {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&self.attributes.to_le_bytes());
        out.extend_from_slice(&self.texture.to_le_bytes());
        out.extend_from_slice(&self.palette.to_le_bytes());
        out.extend_from_slice(&[
            u8::from(self.front),
            u8::from(self.translucent),
            u8::from(self.w_buffer),
            u8::from(self.degenerate),
            self.top,
            self.bottom,
        ]);
    }

    fn unpack(from: &mut Reader<'_>) -> Self {
        Self {
            vertices: std::array::from_fn(|_| from.u16()),
            count: from.u8(),
            depth: std::array::from_fn(|_| from.i32()),
            w: std::array::from_fn(|_| from.i32()),
            attributes: from.u32(),
            texture: from.u32(),
            palette: from.u16(),
            front: from.bool(),
            translucent: from.bool(),
            w_buffer: from.bool(),
            degenerate: from.bool(),
            top: from.u8(),
            bottom: from.u8(),
        }
    }
}

thread_local! {
    static PACKING: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn serialize<S: Serializer, T: Packed>(
    items: &[T],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    PACKING.with_borrow_mut(|out| {
        out.clear();
        out.reserve(items.len() * T::SIZE);
        for item in items {
            item.pack(out);
        }
        serializer.serialize_bytes(out)
    })
}

pub(super) fn deserialize<'de, D: Deserializer<'de>, T: Packed>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    deserializer.deserialize_byte_buf(Items(PhantomData))
}

struct Items<T>(PhantomData<T>);

impl<'de, T: Packed> Visitor<'de> for Items<T> {
    type Value = Vec<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("packed items or a list of items")
    }

    fn visit_bytes<E: Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
        if !bytes.len().is_multiple_of(T::SIZE) {
            return Err(E::invalid_length(bytes.len(), &self));
        }
        let mut from = Reader { bytes };
        Ok((0..bytes.len() / T::SIZE)
            .map(|_| T::unpack(&mut from))
            .collect())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Frame;
    use super::*;

    #[derive(serde::Serialize)]
    struct Listed<'a> {
        polygons: &'a [Polygon],
        vertices: &'a [Vertex],
    }

    fn frame() -> Frame {
        let vertex = |n: i32| Vertex {
            position: [n, -n, 3 * n, i32::MIN + n],
            screen: [n as u16, 191],
            subpixel: [3, 15],
            color: [31, n as u8, 7],
            texcoord: [-(n as i16), i16::MAX],
            clipped: n % 2 == 0,
        };
        let polygon = Polygon {
            vertices: std::array::from_fn(|i| i as u16 * 3),
            count: 4,
            depth: std::array::from_fn(|i| 0x7f_ffff - i as i32),
            w: std::array::from_fn(|i| -(i as i32) << 12),
            attributes: 0x1f00_c0de,
            texture: 0xdead_beef,
            palette: 0x1234,
            front: true,
            translucent: false,
            w_buffer: true,
            degenerate: false,
            top: 2,
            bottom: 3,
        };
        Frame {
            polygons: vec![polygon, Polygon::default()],
            vertices: (0..5).map(vertex).collect(),
        }
    }

    #[test]
    fn a_frame_comes_back_from_packed_bytes() {
        let frame = frame();
        let bytes = rmp_serde::to_vec(&frame).unwrap();
        assert!(bytes.len() < 2 * Polygon::SIZE + 5 * Vertex::SIZE + 16);
        assert_eq!(rmp_serde::from_slice::<Frame>(&bytes).unwrap(), frame);
    }

    #[test]
    fn a_frame_saved_as_lists_still_loads() {
        let frame = frame();
        let listed = Listed {
            polygons: &frame.polygons,
            vertices: &frame.vertices,
        };
        let bytes = rmp_serde::to_vec(&listed).unwrap();
        assert_eq!(rmp_serde::from_slice::<Frame>(&bytes).unwrap(), frame);
    }
}

use std::mem;

pub type Coordinate = i32;
type TileValue = u64;
type TileIndex = u32;
const TILE_SHIFT: u32 = 3;
const TILE_MASK: Coordinate = (1 << TILE_SHIFT) - 1;

struct InternalAddress {
    tile: usize,
    mask: TileValue,
}

#[derive(Clone)]
pub struct Grid {
    size: (Coordinate, Coordinate),
    size_in_tiles: (TileIndex, TileIndex),
    data: Box<[TileValue]>,
}

impl Grid {
    pub fn new(width: Coordinate, height: Coordinate) -> Self {
        assert!(1 << (TILE_SHIFT * 2) == mem::size_of::<TileValue>() * 8);
        let size_in_tiles = (
            ((width - 1) as TileIndex >> TILE_SHIFT) + 1,
            ((height - 1) as TileIndex >> TILE_SHIFT) + 1,
        );
        let data = vec![0; (size_in_tiles.0 * size_in_tiles.1) as usize].into_boxed_slice();
        Self {
            size: (width, height),
            size_in_tiles,
            data,
        }
    }

    fn internal_address(&self, x: Coordinate, y: Coordinate) -> InternalAddress {
        debug_assert!(x >= 0 && x < self.size.0);
        debug_assert!(y >= 0 && y < self.size.1);
        let tile =
            (y >> TILE_SHIFT) as usize * self.size_in_tiles.1 as usize + (x >> TILE_SHIFT) as usize;
        let bit_index = ((y & TILE_MASK) << TILE_SHIFT) + (x & TILE_MASK);
        InternalAddress {
            tile,
            mask: 1 << bit_index,
        }
    }

    pub fn clear(&mut self) {
        for v in self.data.iter_mut() {
            *v = 0;
        }
    }

    pub fn get(&self, x: Coordinate, y: Coordinate) -> bool {
        let ia = self.internal_address(x, y);
        self.data[ia.tile] & ia.mask != 0
    }
    pub fn get_wrapped(&self, x: Coordinate, y: Coordinate) -> bool {
        let ia = self.internal_address(
            if x < 0 {
                x + self.size.0
            } else if x >= self.size.0 {
                x - self.size.0
            } else {
                x
            },
            if y < 0 {
                y + self.size.1
            } else if y >= self.size.1 {
                y - self.size.1
            } else {
                y
            },
        );
        self.data[ia.tile] & ia.mask != 0
    }

    pub fn set(&mut self, x: Coordinate, y: Coordinate) {
        let ia = self.internal_address(x, y);
        self.data[ia.tile] |= ia.mask;
    }

    pub fn _unset(&mut self, x: Coordinate, y: Coordinate) {
        let ia = self.internal_address(x, y);
        self.data[ia.tile] &= !ia.mask;
    }
}

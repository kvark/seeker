use std::num::NonZeroU32;

pub type Coordinate = i32;
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Coordinates {
    pub x: Coordinate,
    pub y: Coordinate,
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub age: NonZeroU32,
    pub avg_breed_age: f32,
    pub avg_velocity: [f32; 2],
}

pub struct Grid {
    size: Coordinates,
    cells: Box<[Option<Cell>]>,
}

pub struct GridAnalysis {
    pub alive_ratio: f32,
}

const NULL_CELL: Option<Cell> = None;

impl Grid {
    pub fn new(size: Coordinates) -> Self {
        let cells = vec![NULL_CELL; size.x as usize * size.y as usize].into_boxed_slice();
        Self { size, cells }
    }

    pub fn size(&self) -> Coordinates {
        self.size
    }

    fn cell_index(&self, x: Coordinate, y: Coordinate) -> usize {
        y.rem_euclid(self.size.y) as usize * (self.size.x as usize)
            + x.rem_euclid(self.size.x) as usize
    }

    pub fn mutate(&mut self, x: Coordinate, y: Coordinate) -> &mut Option<Cell> {
        let index = self.cell_index(x, y);
        self.cells.get_mut(index).unwrap()
    }

    pub fn init(&mut self, x: Coordinate, y: Coordinate) {
        *self.mutate(x, y) = Some(Cell {
            age: NonZeroU32::new(1).unwrap(),
            avg_breed_age: 0.0,
            avg_velocity: [0.0; 2],
        });
    }

    pub fn get(&self, x: Coordinate, y: Coordinate) -> Option<&Cell> {
        let index = self.cell_index(x, y);
        self.cells.get(index).unwrap().as_ref()
    }

    pub fn analyze(&self) -> GridAnalysis {
        let alive: usize = self.cells.iter().filter(|cell| cell.is_some()).count();
        GridAnalysis {
            alive_ratio: alive as f32 / (self.size.x * self.size.y) as f32,
        }
    }
}

use std::num::NonZeroU32;

pub type Coordinate = i32;

#[derive(Clone, Debug)]
pub struct Cell {
    pub age: NonZeroU32,
    pub avg_breed_age: f32,
    pub avg_velocity: [f32; 2],
}

pub struct Grid {
    size: (Coordinate, Coordinate),
    cells: Box<[Option<Cell>]>,
}

const NULL_CELL: Option<Cell> = None;

impl Grid {
    pub fn new(width: Coordinate, height: Coordinate) -> Self {
        let cells = vec![NULL_CELL; width as usize * height as usize].into_boxed_slice();
        Self {
            size: (width, height),
            cells,
        }
    }

    fn cell_index(&self, x: Coordinate, y: Coordinate) -> usize {
        y.rem_euclid(self.size.1) as usize * (self.size.0 as usize)
            + x.rem_euclid(self.size.0) as usize
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
}

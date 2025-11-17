use crossterm::terminal::size;

use crate::error::{ Result };

pub fn get_term_size() -> Result<(u16, u16)> {
    Ok(size()?)
}

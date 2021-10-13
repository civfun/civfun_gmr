use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io;
use std::io::{Cursor, Read, Seek, SeekFrom};
use thiserror::Error;
use tracing::trace;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid header in save.")]
    BadHeader,

    #[error("IoError")]
    IoError(#[from] io::Error),

    #[error("Utf8 Error")]
    Utf8Error(#[from] std::str::Utf8Error),
}

type Result<T> = std::result::Result<T, Error>;

struct Civ5SaveReader<'a> {
    cursor: Cursor<&'a [u8]>,
}

impl<'a> Civ5SaveReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        let cursor = Cursor::new(bytes);
        Civ5SaveReader { cursor }
    }

    pub fn parse(&mut self) -> Result<Civ5Save> {
        if self.exact(4)? != "CIV5".as_bytes() {
            return Err(Error::BadHeader);
        }
        let save = self.u32()?;
        let game = self.string()?;
        let build = self.string()?;
        let turn = self.u32()?;
        self.exact(1)?;
        let starting_civ = self.string()?;
        let handicap = self.string()?;
        let era = self.string()?;
        let current_era = self.string()?;
        let game_speed = self.string()?;
        let world_size = self.string()?;
        let map_script = self.string()?;
        trace!(
            ?save,
            ?game,
            ?turn,
            ?starting_civ,
            // handicap,
            // era,
            // current_era,
            // game_speed,
            // world_size,
            // map_script
        );

        // Chunk 2. Just player names?
        self.chunk(2)?;
        let mut player_names = vec![];
        loop {
            let player_name = self.string()?;
            trace!(?player_name);
            if player_name.is_empty() {
                break;
            }
            player_names.push(player_name);
        }

        // Chunk 3.
        self.chunk(1)?;
        trace!("Chunk 3");
        dbg!(self.u32()?);
        dbg!(self.u32()?);
        dbg!(self.u32()?);
        dbg!(self.u32()?);

        Ok((Civ5Save { current_turn: 123 }))
    }

    fn exact(&mut self, size: usize) -> Result<Vec<u8>> {
        let mut s = vec![0u8; size];
        self.cursor.read_exact(&mut s)?;
        Ok(s)
    }

    fn string(&mut self) -> Result<String> {
        let size = self.u32()? as usize;
        let s = self.exact(size)?;
        Ok(std::str::from_utf8(&s)?.into())
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(self.cursor.read_u32::<LittleEndian>()?)
    }

    /// Seek forward until the bytes match. It will seek past the end of bytes.
    fn seek_past_match(&mut self, bytes: &[u8]) -> Result<()> {
        // This is probably pretty inefficient, as we're allocating at each byte.
        self.cursor.seek(SeekFrom::Current(1))?;
        loop {
            let found = self.exact(bytes.len())?;
            if found == bytes {
                return Ok(());
            }
            // Seek back the size of bytes, minus one so that we've advanced to the next byte.
            self.cursor
                .seek(SeekFrom::Current(-(bytes.len() as i64 - 1)))?;
        }
    }

    fn chunk(&mut self, count: usize) -> Result<()> {
        for _ in 0..count {
            self.seek_past_match(&[0x40, 0, 0, 0])?;
        }
        Ok(())
    }
}

struct Civ5Save {
    current_turn: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_env_log::test]
    fn test() {
        let data = include_bytes!("../saves/Casimir III_0028 BC-2320.Civ5Save");
        let mut c = Civ5SaveReader::new(data);
        let save = c.parse().unwrap();
        assert_eq!(save.current_turn, 27);
    }
}

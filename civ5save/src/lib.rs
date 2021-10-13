use anyhow::anyhow;
use byteorder::{LittleEndian, ReadBytesExt};
use pretty_hex::pretty_hex;
use std::convert::{TryFrom, TryInto};
use std::fmt::{Debug, Formatter};
use std::io;
use std::io::{Cursor, Read, Seek, SeekFrom};
use tracing::{debug, instrument, trace};

// use thiserror::Error;

// #[derive(Error, Debug)]
// pub enum Error {
//     #[error("Invalid header in save.")]
//     BadHeader,
//
//     #[error("Unknown player type {0}.")]
//     UnknownPlayerType(u32),
//
//     #[error("IoError")]
//     IoError(
//         #[from]
//         #[backtrace]
//         io::Error,
//     ),
//
//     #[error("Utf8 Error")]
//     Utf8Error(
//         #[from]
//         #[backtrace]
//         std::str::Utf8Error,
//     ),
// }

// type Result<T> = std::result::Result<T, Error>;
type Result<T> = anyhow::Result<T, anyhow::Error>;
type Error = anyhow::Error;

#[derive(Clone, Debug)]
pub enum PlayerType {
    AI = 1,
    Dead = 2,
    Human = 3,
    None = 4,
}

impl TryFrom<u32> for PlayerType {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        use PlayerType::*;
        Ok(match value {
            1 => AI,
            2 => Dead,
            3 => Human,
            4 => None,
            v => return Err(anyhow!("UnknownPlayerType {}", v)),
        })
    }
}

#[derive(Clone)]
struct Chunk {
    id: usize,
    offset: u64,
    size: u64,
    data: Vec<u8>,
}

impl Debug for Chunk {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Chunk {{ id: {} offset: {} size: {} }}",
            self.id, self.offset, self.size
        )
    }
}

#[derive(Clone, Debug)]
struct Header {
    save: u32,
    game: String,
    build: String,
    turn: u32,
    starting_civ: String,
    handicap: String,
    era: String,
    current_era: String,
    game_speed: String,
    world_size: String,
    map_script: String,
}

#[derive(Clone, Debug)]
struct Player {
    name: String,
    player_type: PlayerType,
    // civ: String,
    // leader: String,
}

struct Civ5SaveReader<'a> {
    cursor: Cursor<&'a [u8]>,
    chunks: Vec<Chunk>,
}

impl<'a> Civ5SaveReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        let cursor = Cursor::new(bytes);
        Civ5SaveReader {
            cursor,
            chunks: vec![],
        }
    }

    pub fn parse(&mut self) -> Result<Civ5Save> {
        if self.exact(4)? != "CIV5".as_bytes() {
            return Err(anyhow!("Bad header"));
            // return Err(Error::BadHeader);
        }

        let header = self.header()?;
        debug!(?header);

        self.load_chunks()?;
        // self.dump_chunks()?;

        self.chunk(1)?;
        let player_names = self.strings()?;
        debug!(?player_names);

        self.chunk(2)?;
        let mut player_types: Vec<PlayerType> = vec![];
        for _ in 0..player_names.len() {
            player_types.push(self.u32()?.try_into()?);
        }
        debug!(?player_types);

        // self.chunk(6)?;
        // let civs = self.strings()?;
        // debug!(?civs);
        //
        // self.chunk(7)?;
        // let leaders = self.strings()?;
        // debug!(?leaders);

        let mut players = vec![];
        for i in 0..player_names.len() {
            players.push(Player {
                name: player_names[i].clone(),
                player_type: player_types[i].clone(),
                // civ: civs[i].clone(),
                // leader: leaders[i].clone(),
            })
        }

        Ok(Civ5Save {
            header,
            players,
            chunks: self.chunks.clone(),
        })
    }

    fn header(&mut self) -> Result<Header> {
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
        Ok(Header {
            save,
            game,
            build,
            turn,
            starting_civ,
            handicap,
            era,
            current_era,
            game_speed,
            world_size,
            map_script,
        })
    }

    fn strings(&mut self) -> Result<Vec<String>> {
        let mut v = vec![];
        loop {
            let s = self.string()?;
            trace!(?s, "Reading string for strings");
            if s.is_empty() {
                return Ok(v);
            }
            v.push(s)
        }
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
        // This is probably pretty inefficient, as we're allocating at each byte position.
        // Computers are fast anyway right?
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

    #[instrument(skip(self))]
    fn load_chunks(&mut self) -> Result<()> {
        let chunk_boundary = &[0x40, 0, 0, 0];
        self.chunks = vec![];
        self.cursor.seek(SeekFrom::Start(0))?;
        loop {
            let offset = self.cursor.position();
            self.seek_past_match(chunk_boundary)?;
            let new_position = self.cursor.position();
            let end_offset = new_position - chunk_boundary.len() as u64;
            let size = end_offset - offset;

            // Grab the chunk data.
            self.cursor.set_position(offset);
            let mut data = vec![0u8; size as usize];
            self.cursor.read_exact(&mut data)?;

            let id = self.chunks.len();
            let info = Chunk {
                id,
                offset: new_position,
                size,
                data,
            };
            trace!(chunk = ?id, ?info);
            self.chunks.push(info);
            if self.chunks.len() == 31 {
                return Ok(());
            }
        }
    }

    fn dump_chunks(&mut self) -> Result<()> {
        for chunk in &self.chunks {
            println!("Chunk {} {:?} {:?}", chunk.id, chunk.offset, chunk.size);
            println!("{}", pretty_hex(&chunk.data));
        }
        Ok(())
    }

    fn chunk(&mut self, chunk: usize) -> Result<()> {
        let info = &self.chunks[chunk];
        trace!(?chunk, ?info);
        self.cursor.seek(SeekFrom::Start(info.offset))?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Civ5Save {
    header: Header,
    players: Vec<Player>,
    chunks: Vec<Chunk>,
}

impl Civ5Save {
    /// This is pretty simple. Go through each chunk and compare by byte.
    ///
    /// The more it's wrong, the higher the result.
    fn difference_score(&self, other: &Civ5Save) -> Result<u32> {
        let mut diff = 0u32;
        for (chunk_idx, chunk) in self.chunks.iter().enumerate() {
            let other_chunk = &other.chunks[chunk_idx];
            for (data_idx, data) in chunk.data.iter().enumerate() {
                match other_chunk.data.get(data_idx) {
                    None => {
                        diff += 1;
                    }
                    Some(b) => {
                        if data != &other_chunk.data[data_idx] {
                            diff += 1;
                        }
                    }
                }
            }
        }
        Ok(diff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::fs::File;
    use std::path::PathBuf;
    use tracing::{error, info};

    fn load(path: &str) -> Civ5Save {
        let mut fp = File::open(path).unwrap();
        let mut buffer = vec![];
        fp.read_to_end(&mut buffer).unwrap();
        let mut c = Civ5SaveReader::new(&buffer);
        c.parse().unwrap()
    }

    #[test_env_log::test]
    fn sanity() {
        let save = load("saves/Casimir III_0028 BC-2320.Civ5Save".into());
        assert_eq!(save.header.turn, 28);
    }

    #[test_env_log::test]
    fn same() {
        let save_a = load("saves/Casimir III_0028 BC-2320.Civ5Save".into());
        let save_b = save_a.clone();
        assert_eq!(save_a.difference_score(&save_b).unwrap(), 0);
    }

    #[test_env_log::test]
    fn small_diff() {
        let save_a = load("saves/Casimir III_0005 BC-3700.Civ5Save");
        let save_b = load("saves/Casimir III_0028 BC-2320.Civ5Save");
        let save_c = load("saves/Casimir III_0029 BC-2260.Civ5Save");
        assert_eq!(save_a.difference_score(&save_b).unwrap(), 11);
        assert_eq!(save_a.difference_score(&save_c).unwrap(), 9);
        assert_eq!(save_b.difference_score(&save_c).unwrap(), 9);
    }

    #[test_env_log::test]
    fn big_diff() {
        let saves = [
            "saves/Casimir III_0029 BC-2260.Civ5Save",
            "saves/Ahmad al-Mansur_0054 BC-0840.Civ5Save",
            "saves/Elizabeth_0437 AD-2017.Civ5Save",
            "saves/Genghis Khan_0138 AD-1480.Civ5Save",
            "saves/Harun al-Rashid_0179 AD-1770.Civ5Save",
            "saves/Pocatello_0164 AD-1040.Civ5Save",
            "saves/Suleiman_0021 BC-2740.Civ5Save",
        ];
        for a in &saves {
            for b in &saves {
                if a == b {
                    continue;
                }
                info!(?a, "Loading");
                let save_a = load(a);
                info!(?b, "Loading");
                let save_b = load(b);
                let diff = save_a.difference_score(&save_b).unwrap();
                info!(?a, ?b, ?diff, "Comparing");
                assert!(diff > 500);
            }
        }
    }
}

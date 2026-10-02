mod crc;
mod dict;
mod fnv;
mod misc;
mod xxh;

pub use crc::{Crc, BZIP2, CASTAGNOLI, IEEE, JAMCRC, MPEG2};
pub use dict::Dict;
pub use fnv::{fnv64, Fnv, FNV1A};
pub use misc::{adler32, djb2, murmur3, oaat, sdbm};
pub use xxh::{xxh32, xxh64};

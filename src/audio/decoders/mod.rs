pub mod flac;
pub mod wav;
pub mod alac;
pub mod mp3;
pub mod ogg;
pub mod m4a;

pub use flac::FlacDecoder;
pub use wav::WavDecoder;
pub use alac::AlacDecoder;
pub use mp3::Mp3Decoder;
pub use ogg::OggDecoder;
pub use m4a::M4aDecoder;

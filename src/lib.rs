#![doc = include_str!("../README.md")]

mod huffman;

pub use huffman::Error as HuffmanError;

#[derive(Debug)]
pub enum Error {
    Bincode(bincode::Error),
    Huffman(HuffmanError),
}

impl From<bincode::Error> for Error {
    fn from(error: bincode::Error) -> Self {
        Error::Bincode(error)
    }
}
impl From<huffman::Error> for Error {
    fn from(error: huffman::Error) -> Self {
        Error::Huffman(error)
    }
}

/// Encode and compress a value to a vector of bytes, which includes the metadata for decoding
pub fn huff<T>(value: &T) -> Result<Vec<u8>, Error>
where
    T: serde::Serialize,
{
    let bincoded_bytes = bincode::serialize(value)?;
    Ok(huffman::encode(&bincoded_bytes)?)
}

/// Decode a buffer encoded by this library into a DeserializeOwned type
pub fn puff<'a, T>(bytes: &'a [u8]) -> Result<T, Error>
where
    T: serde::de::DeserializeOwned,
{
    let bincoded_bytes = huffman::decode(bytes)?;
    Ok(bincode::deserialize(&bincoded_bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let int: u32 = 42;
        let string: String = "this is a string".to_owned();
        let tuple: (bool, u8) = (true, 255);
        let empty_vec: Vec<u8> = vec![];

        {
            let encoded = huff(&empty_vec).unwrap();
            assert_eq!(puff::<Vec<u8>>(&encoded).unwrap(), empty_vec);
        }

        {
            let encoded = huff(&int).unwrap();
            assert_eq!(puff::<u32>(&encoded).unwrap(), int);
        }
        {
            let encoded = huff(&string).unwrap();
            assert_eq!(puff::<String>(&encoded).unwrap(), string);
        }
        {
            let encoded = huff(&tuple).unwrap();
            assert_eq!(puff::<(bool, u8)>(&encoded).unwrap(), tuple);
        }
        {
            let encoded = huff(&empty_vec).unwrap();
            let decoded: Vec<u8> = puff(&encoded).unwrap();
            assert_eq!(decoded, empty_vec);
        }
    }

    #[test]
    fn compress_lorem() {
        let plaintext = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";
        let encoded = huff(&plaintext).unwrap();

        // println!(
        //     "binary size: {}\nencoded size: {}",
        //     plaintext.as_bytes().len(),
        //     encoded.len()
        // );
        // println!("{}", puff::<String>(&encoded).unwrap());

        assert!(encoded.len() < plaintext.as_bytes().len());
    }

    #[test]
    fn roundtrip_custom() {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        struct User {
            id: u64,
            name: String,
            is_active: bool,
        }

        let user = User {
            id: 42,
            name: "Alice".to_string(),
            is_active: true,
        };

        let compressed = huff(&user).unwrap();

        let decompressed: User = puff(&compressed).unwrap();
        assert_eq!(user, decompressed);
    }

    #[test]
    fn test_empty() {
        let empty: Vec<u8> = vec![];

        let decompressed: Result<Vec<u8>, Error> = puff(&empty);
        println!("{:?}", decompressed);
        assert!(matches!(
            decompressed,
            Err(Error::Huffman(huffman::Error::NoData))
        ));
    }

    #[test]
    fn test_invalid() {
        let message = "Hello, world!";
        let decompressed: Result<Vec<u8>, Error> = puff(message.as_bytes());

        assert!(matches!(
            decompressed,
            Err(Error::Huffman(huffman::Error::FailedToDecodeHuffmanTree))
        ));
    }

    // #[test]
    // fn test_statistics() {
    //     let short_message = "Hello, world!";
    //     let long_message = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

    //     let compressed = huff(&short_message).unwrap();
    //     // Short messages may not compress well, since the overhead of the huffman tree will be significant compared to the message size
    //     println!(
    //         "Short message uncompressed: {}, compressed: {}, ratio: {:.2}",
    //         short_message.len(),
    //         compressed.len(),
    //         compressed.len() as f64 / short_message.len() as f64,
    //     );

    //     let compressed = huff(&long_message).unwrap();
    //     // Long messages should compress well, since the overhead of the huffman tree will be amortized over the message size
    //     println!(
    //         "Long message uncompressed: {}, compressed: {}, ratio: {:.2}",
    //         long_message.len(),
    //         compressed.len(),
    //         compressed.len() as f64 / long_message.len() as f64,
    //     );

    //     assert!(false);
    // }
}

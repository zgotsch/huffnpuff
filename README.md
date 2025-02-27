# Huffnpuff

A lightweight Huffman coding implementation in Rust.

## Description

huffnpuff is a toy Rust library written mostly for educational purposes. It provides a simple API for compressing and decompressing data using Huffman coding, a simple compression algorithm.

Caveats:

- Deserialized types must be `serde::DeserializeOwned`, since references into the compressed data are not typically useful, and enforcing this constraint removes the need to track the decompression buffer's lifetime.

The library integrates with Serde for seamless compression of serializable Rust types, allowing you to compress arbitrary data structures without manual conversions.

## Usage

Add the library to your `Cargo.toml`:

```toml
[dependencies]
huffnpuff = "0.1.0"
```

### Basic Example

```rust
use huffnpuff::{huff, puff};

// Compress a string
let message = "Hello, world!";
let compressed = huff(&message).unwrap();

// Decompress back to original
let decompressed: String = puff(&compressed).unwrap();
assert_eq!(message, decompressed);
```

### With Custom Types

```rust
use serde::{Serialize, Deserialize};
use huffnpuff::{huff, puff};

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

// Compress the struct
let compressed = huff(&user).unwrap();

// Decompress back to struct
let decompressed: User = puff(&compressed).unwrap();
assert_eq!(user, decompressed);
```

### Compression Ratio

```rust
use huffnpuff::{huff, puff};

// Compress a string
let short_message = "Hello, world!";
let long_message = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum.";

let compressed = huff(&short_message).unwrap();
// Short messages may not compress well, since the overhead of the huffman tree will be significant compared to the message size
println!(
   "Short message uncompressed: {}, compressed: {}, ratio: {:.2}",
   short_message.len(),
   compressed.len(),
   compressed.len() as f64 / short_message.len() as f64,
);
// Short message uncompressed: 13, compressed: 24, ratio: 1.85

let compressed = huff(&long_message).unwrap();
// Long messages should compress well, since the overhead of the huffman tree will be amortized over the message size
println!(
   "Long message uncompressed: {}, compressed: {}, ratio: {:.2}",
   long_message.len(),
   compressed.len(),
   compressed.len() as f64 / long_message.len() as f64,
);
// Long message uncompressed: 445, compressed: 278, ratio: 0.62
```

## Further Work

- **API changes**: The current API does not expose the huffman tree, it is always encoded in the compressed data. This is not ideal for some use cases, where the tree could be shared between multiple compressed data. A future version could expose the tree for reuse.
- **Optimizations**: The current implementation is not optimized for performance, and probably uses both more space and does more work than is necessary. Additionally, unaligned bit reads/writes are used, which may be slow on some platforms.
- **Streaming API**: Add support for compressing/decompressing streams rather than only in-memory buffers.
- **Error handling**: The current error handling is minimal, and operations may panic.

## License

MIT

use bitvec::{field::BitField, order::Lsb0, view::BitView};
use std::collections::{BTreeMap, HashMap};

type BitSlice = bitvec::prelude::BitSlice<u8, Lsb0>;
type BitVec = bitvec::prelude::BitVec<u8, Lsb0>;

#[derive(Debug)]
pub enum Error {
    /// No data was provided to the encoding or decoding function
    NoData,
    /// It was not possible to decode the huffman tree from the provided data. Maybe this data was not encoded by huffnpuff?
    FailedToDecodeHuffmanTree,
}

pub(crate) fn encode(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    if bytes.is_empty() {
        return Err(Error::NoData);
    }

    let tree = Node::tree_for_message(bytes);
    let mut bits = tree.serialize();
    let message = tree.encode(bytes);

    bits.extend_from_bitslice(&message);
    bits.set_uninitialized(false);
    Ok(bits.into_vec())
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    if bytes.is_empty() {
        return Err(Error::NoData);
    }

    let bits = bytes.view_bits();
    if let Some((tree, bits)) = Node::deserialize(bits) {
        return Ok(tree.decode(bits));
    } else {
        return Err(Error::FailedToDecodeHuffmanTree);
    }
}

#[derive(Debug)]
enum Node {
    Inner {
        count: u32,
        left: Box<Node>,
        right: Box<Node>,
    },
    Leaf {
        count: u32,
        value: u8,
    },
}

impl Node {
    fn join(left: Self, right: Self) -> Self {
        Node::Inner {
            count: left.count() + right.count(),
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn new(count: u32, value: u8) -> Self {
        Self::Leaf { count, value }
    }

    /// Invariant: The tree returned by this constructor will always have at least one inner node.
    /// Calling this function with an empty slice is an error, and will panic.
    fn tree_for_message(bytes: &[u8]) -> Self {
        let frequencies = bytes.iter().fold(HashMap::new(), |mut acc, &byte| {
            *acc.entry(byte).or_insert(0) += 1;
            acc
        });

        let mut nodes: Vec<Node> = frequencies
            .into_iter()
            .map(|(value, count)| Node::new(count, value))
            .collect();

        if nodes.len() == 1 {
            // HACK(zgotsch): If there is only one value in the message, we add another fake node so we
            // know whether to use 0 or 1 for the value
            nodes.push(Node::new(0, !bytes[0]))
        }

        while nodes.len() > 1 {
            nodes.sort_by_key(|node| node.count());
            let left = nodes.remove(0);
            let right = nodes.remove(0);
            nodes.push(Node::join(left, right));
        }

        nodes.pop().unwrap()
    }

    fn count(&self) -> u32 {
        match self {
            Self::Inner { count, .. } => *count,
            Self::Leaf { count, .. } => *count,
        }
    }

    fn encode(&self, bytes: &[u8]) -> BitVec {
        let codebook = {
            // Precompute a codebook for the tree
            let mut codebook = BTreeMap::new();
            fn traverse(codebook: &mut BTreeMap<u8, BitVec>, path: &mut BitVec, node: &Node) {
                match node {
                    Node::Leaf { value, .. } => {
                        codebook.insert(*value, path.clone());
                    }
                    Node::Inner { left, right, .. } => {
                        path.push(false);
                        traverse(codebook, path, left);
                        path.pop();
                        path.push(true);
                        traverse(codebook, path, right);
                        path.pop();
                    }
                }
            }

            let mut path = BitVec::new();
            traverse(&mut codebook, &mut path, self);

            codebook
        };

        let mut bits = BitVec::new();
        for byte in bytes {
            if let Some(encoded) = codebook.get(byte) {
                bits.extend_from_bitslice(encoded);
            } else {
                panic!("missing value in codebook");
            }
        }
        bits
    }

    fn decode(&self, bits: &BitSlice) -> Vec<u8> {
        let mut ret = Vec::new();

        let mut cursor = self;
        // no single node trees allowed
        assert!(matches!(cursor, Node::Inner { .. }));

        // we're going to peel off one bit at a time, traversing the tree til we reach a leaf
        for bit in bits {
            match cursor {
                Node::Inner { left, right, .. } => match *bit {
                    false => {
                        cursor = left.as_ref();
                    }
                    true => {
                        cursor = right.as_ref();
                    }
                },
                Node::Leaf { .. } => {
                    panic!("shouldn't have a leaf node here!");
                }
            }

            match cursor {
                Node::Leaf { value, .. } => {
                    ret.push(*value);
                    cursor = &self;
                }
                _ => {}
            }
        }

        ret
    }

    /// A compact representation of a huffman encoding tree. A preorder traversal indicating whether
    /// nodes are leaves or not, followed by the value data.
    fn serialize(&self) -> BitVec {
        // traverse the tree
        fn traverse(tree: &mut BitVec, values: &mut Vec<u8>, n: &Node) {
            match n {
                Node::Leaf { value, .. } => {
                    tree.push(true);
                    values.push(*value)
                }
                Node::Inner { left, right, .. } => {
                    tree.push(false);
                    traverse(tree, values, left);
                    traverse(tree, values, right);
                }
            }
        }

        let mut tree = BitVec::new();
        let mut values = Vec::<u8>::new();
        traverse(&mut tree, &mut values, self);

        tree.extend(values.view_bits::<Lsb0>());

        tree
    }

    /// Decode a tree from the prefix of a bitslice
    fn deserialize(bits: &BitSlice) -> Option<(Self, &BitSlice)> {
        fn helper<'a>(leaf_count: &mut u32, bits: &'a BitSlice) -> Option<(Node, &'a BitSlice)> {
            let (is_leaf, rest) = bits.split_first()?;
            if *is_leaf {
                *leaf_count += 1;
                // No counts in the rehydrated tree, no values yet
                return Some((Node::Leaf { count: 0, value: 0 }, rest));
            }

            let (left, rest) = helper(leaf_count, rest)?;
            let (right, rest) = helper(leaf_count, rest)?;
            let node = Node::Inner {
                count: 0,
                left: Box::new(left),
                right: Box::new(right),
            };
            Some((node, rest))
        }

        let mut leaf_count = 0;
        let (mut tree, remaining) = helper(&mut leaf_count, bits)?;

        if (leaf_count * 8) > remaining.len() as u32 {
            // Error, there isn't enough data to fill out the leaf nodes
            return None;
        }

        // traverse the new tree, deserializing byte values from the stream
        fn traverse<'a>(bits: &'a BitSlice, node: &mut Node) -> &'a BitSlice {
            match node {
                Node::Leaf { value, .. } => {
                    let (value_bits, rest) = bits.split_at(8);
                    *value = value_bits.load();
                    return rest;
                }
                Node::Inner { left, right, .. } => {
                    let rest = traverse(bits, left);
                    let rest = traverse(rest, right);
                    return rest;
                }
            }
        }

        let remaining = traverse(remaining, &mut tree);
        if matches!(tree, Node::Leaf { .. }) {
            // Error, the tree should have at least one inner node
            return None;
        }
        return Some((tree, remaining));
    }
}

#[cfg(test)]
mod tests {
    use bitvec::bits;

    use super::*;

    #[test]
    fn test_bug_padding_decoded_as_data() {
        /*
         * A handcrafted example where the padding bits are interpreted as data
         *
         * The tree is:
         * o
         * /\
         * o c
         * /\
         * a b
         *
         * Corresponding to a codebook of:
         * a: 00
         * b: 01
         * c: 1
         *
         * The encoding of the huffman tree is thus:
         * 0 0 1 1 1
         * Followed by byte values:
         * a b c
         *
         * Thus the total encoded tree is 5 + 24 = 29 bits long.
         *
         * It's important that the total encoded message leaves empty bits at the end, so it should
         * be 8n + 1 bits. Thus, a simple 4 bit message is chosen: 00 00, which corresponds to "aa".
         *
         * a: 0x61 0b0110_0001
         * b: 0x62 0b0110_0010
         * c: 0x63 0b0110_0011
         *
         * Thus the whole message is 33 bits long:
         * 0b00111_01100001_01100010_01100011_00_00_1111111
         */
        let tree = bits![u8, Lsb0; 0, 0, 1, 1, 1];
        let values = vec![0x61 as u8, 0x62, 0x63];
        let message = bits![u8, Lsb0; 0, 0, 0, 0];
        let padding = bits![u8, Lsb0; 1, 1, 1, 1, 1, 1, 1];

        let mut bytes = BitVec::new();
        bytes.extend_from_bitslice(&tree);
        bytes.extend_from_bitslice(&values.view_bits::<Lsb0>());
        bytes.extend_from_bitslice(&message);
        assert_eq!(bytes.len(), 33);

        bytes.extend_from_bitslice(&padding);
        assert_eq!(bytes.len(), 40);

        // let bytes = vec![
        //     0b00111_011,
        //     0b00001_011,
        //     0b00010_011,
        //     0b00011_00_0,
        //     // padding
        //     0b0_11111111,
        // ];
        dbg!(&bytes);

        let value = vec![0x61, 0x61];
        let decoded = decode(&bytes.into_vec()).unwrap();
        assert_eq!(dbg!(decoded), value);
    }
}

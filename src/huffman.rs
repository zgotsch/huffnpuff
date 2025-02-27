use bitvec::{field::BitField, order::Lsb0, view::BitView};
use std::collections::HashMap;

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

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
enum HuffmanValue {
    Symbol(u8),
    EndOfMessage,
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
        value: HuffmanValue,
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

    fn new(count: u32, value: HuffmanValue) -> Self {
        Self::Leaf { count, value }
    }

    /// Invariant: The tree returned by this constructor will always have at least one inner node.
    /// Calling this function with an empty slice is an error, and will panic.
    fn tree_for_message(bytes: &[u8]) -> Self {
        assert!(!bytes.is_empty());

        let frequencies = bytes.iter().fold(HashMap::new(), |mut acc, &byte| {
            *acc.entry(byte).or_insert(0) += 1;
            acc
        });

        let mut nodes: Vec<Node> = frequencies
            .into_iter()
            .map(|(value, count)| Node::new(count, HuffmanValue::Symbol(value)))
            .collect();

        // In addition to giving us a way to mark EOM, this also ensures we have an inner node
        nodes.push(Node::new(0, HuffmanValue::EndOfMessage));

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
        let (codebook, eom_bitvec) = {
            // Precompute a codebook for the tree
            let mut codebook: HashMap<u8, BitVec> = HashMap::new();
            let mut eom_bitvec: Option<BitVec> = None;
            fn traverse(
                codebook: &mut HashMap<u8, BitVec>,
                eom_bitvec: &mut Option<BitVec>,
                path: &mut BitVec,
                node: &Node,
            ) {
                match node {
                    Node::Leaf { value, .. } => match value {
                        HuffmanValue::Symbol(s) => {
                            codebook.insert(*s, path.clone());
                        }
                        HuffmanValue::EndOfMessage => {
                            *eom_bitvec = Some(path.clone());
                        }
                    },
                    Node::Inner { left, right, .. } => {
                        path.push(false);
                        traverse(codebook, eom_bitvec, path, left);
                        path.pop();
                        path.push(true);
                        traverse(codebook, eom_bitvec, path, right);
                        path.pop();
                    }
                }
            }

            let mut path = BitVec::new();
            traverse(&mut codebook, &mut eom_bitvec, &mut path, self);

            (codebook, eom_bitvec)
        };

        let mut bits = BitVec::new();
        for byte in bytes {
            if let Some(encoded) = codebook.get(byte) {
                bits.extend_from_bitslice(encoded);
            } else {
                panic!("missing value in codebook");
            }
        }

        // EOM
        bits.extend_from_bitslice(&eom_bitvec.expect("Missing EOM bitvec"));

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

            // If we have a leaf, save that value and reset the cursor state
            if let Node::Leaf { value, .. } = cursor {
                match value {
                    HuffmanValue::EndOfMessage => {
                        return ret;
                    }
                    HuffmanValue::Symbol(s) => {
                        ret.push(*s);
                        cursor = &self;
                    }
                }
            }
        }

        // If we've gotten here, we must have run out of bits without reaching EOM. This probably
        // indicates that there was only a partial message. It's perhaps best to return what we
        // have, since there's no affordance in our API for a result + error.
        ret
    }

    /// A compact representation of a huffman encoding tree. A preorder traversal indicating whether
    /// nodes are leaves or not, followed by the value data.
    fn serialize(&self) -> BitVec {
        // traverse the tree
        fn traverse(tree: &mut BitVec, values: &mut Vec<HuffmanValue>, n: &Node) {
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
        let mut values = Vec::<HuffmanValue>::new();
        traverse(&mut tree, &mut values, self);

        // Append the symbol values
        for value in values {
            // This is an extended representation, which takes 9 bits. The most significant bit
            // is 1 if the value is EOM, and 0 otherwise
            match value {
                HuffmanValue::EndOfMessage => {
                    tree.push(true);
                    tree.extend_from_bitslice(0u8.view_bits::<Lsb0>());
                }
                HuffmanValue::Symbol(s) => {
                    tree.push(false);
                    tree.extend_from_bitslice(s.view_bits::<Lsb0>());
                }
            }
        }

        tree
    }

    const SYMBOL_SIZE: usize = 9;
    /// Decode a tree from the prefix of a bitslice
    fn deserialize(bits: &BitSlice) -> Option<(Self, &BitSlice)> {
        fn helper<'a>(leaf_count: &mut usize, bits: &'a BitSlice) -> Option<(Node, &'a BitSlice)> {
            let (is_leaf, rest) = bits.split_first()?;
            if *is_leaf {
                *leaf_count += 1;
                // No counts in the rehydrated tree, no values yet
                return Some((
                    Node::Leaf {
                        count: 0,
                        value: HuffmanValue::Symbol(0),
                    },
                    rest,
                ));
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

        let mut leaf_count: usize = 0;
        let (mut tree, remaining) = helper(&mut leaf_count, bits)?;

        if (leaf_count * Self::SYMBOL_SIZE) > remaining.len() {
            // Error, there isn't enough data to fill out the leaf nodes
            return None;
        }

        let mut seen_eom = false;
        // traverse the new tree, deserializing byte values from the stream
        fn traverse<'a>(bits: &'a BitSlice, seen_eom: &mut bool, node: &mut Node) -> &'a BitSlice {
            match node {
                Node::Leaf { value, .. } => {
                    let (value_bits, rest) = bits.split_at(Node::SYMBOL_SIZE);
                    let (is_eom, value_bits) = value_bits.split_first().unwrap();
                    if *is_eom {
                        *seen_eom = true;
                        *value = HuffmanValue::EndOfMessage
                    } else {
                        *value = HuffmanValue::Symbol(value_bits.load());
                    }
                    return rest;
                }
                Node::Inner { left, right, .. } => {
                    let rest = traverse(bits, seen_eom, left);
                    let rest = traverse(rest, seen_eom, right);
                    return rest;
                }
            }
        }

        let remaining = traverse(remaining, &mut seen_eom, &mut tree);
        if !seen_eom {
            // Error, the tree is required to have an EOM
            return None;
        }
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
         * a EOM
         *
         * Corresponding to a codebook of:
         * a: 00
         * EOM: 01
         * c: 1
         *
         * The encoding of the huffman tree is thus:
         * 0 0 1 1 1
         * Followed by byte values:
         * 0b0[a]_1[EOM]_0c
         *
         * Thus the total encoded tree is 5 + (9 * 3 = 27) = 32 bits long.
         *
         * It's important that the total encoded message leaves empty bits at the end, so it should
         * be 8n + 1 bits. Thus, a simple 9 bit message is chosen: 00 00 00 1 01, which corresponds to "aaac[EOM]".
         *
         * a: 0x61 0b0110_0001
         * c: 0x63 0b0110_0011
         *
         * Thus the whole message is 41 (48 including padding) bits long:
         * 0b00111_001100001_100000000_001100011_00_00_00_1_01_1111111
         */
        let tree = bits![u8, Lsb0; 0, 0, 1, 1, 1];
        let message = bits![u8, Lsb0; 0, 0, 0, 0, 0, 0, 1, 0, 1];
        let padding = bits![u8, Lsb0; 1, 1, 1, 1, 1, 1, 1];

        let mut bytes = BitVec::new();
        bytes.extend_from_bitslice(&tree);

        let values = vec![Some(0x61 as u8), None, Some(0x63)];
        for value in values {
            if let Some(v) = value {
                bytes.push(false);
                bytes.extend_from_bitslice(v.view_bits::<Lsb0>());
            } else {
                bytes.push(true);
                bytes.extend_from_bitslice(0u8.view_bits::<Lsb0>());
            }
        }

        bytes.extend_from_bitslice(&message);
        assert_eq!(bytes.len(), 41);

        bytes.extend_from_bitslice(&padding);
        assert_eq!(bytes.len(), 48);

        dbg!(&bytes);

        let value = vec![0x61, 0x61, 0x61, 0x63];
        let decoded = decode(&bytes.into_vec()).unwrap();
        assert_eq!(dbg!(decoded), value);
    }
}

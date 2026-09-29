use crate::{index::{Index, IndexRow}, pdb::PalmDatabase};
use lexicon_core::{Alias, Encoding, Error, Result};
use std::collections::BTreeMap;

/// A bounded byte-edit interpreter. Instructions operate before text decoding.
pub fn apply_rule(base: &[u8], rule: &[u8], cap: usize) -> Result<Vec<u8>> {
    #[derive(Clone, Copy, PartialEq)] enum Direction { Left, Right, Hold }
    let mut word = base.to_vec();
    let mut cursor = word.len();
    let mut direction = Direction::Left;
    let mut insert = true;
    for (i, &byte) in rule.iter().enumerate() {
        if byte == 0 {
            if rule[i..].iter().any(|&b| b != 0) { return Err(Error::Malformed("data follows inflection terminator".into())); }
            break;
        }
        if (1..=4).contains(&byte) {
            insert = byte <= 2;
            let next = if byte == 2 || byte == 3 { Direction::Left } else { Direction::Right };
            if direction != Direction::Hold && next != direction { cursor = if next == Direction::Left { word.len() } else { 0 }; }
            direction = next;
        } else if (11..=19).contains(&byte) {
            if direction == Direction::Right { cursor = word.len(); }
            cursor = cursor.checked_sub(usize::from(byte - 10)).ok_or_else(|| Error::Malformed("inflection cursor before start".into()))?;
            direction = Direction::Hold;
        } else if byte <= 19 {
            return Err(Error::Unsupported(format!("inflection opcode {byte}")));
        } else if insert {
            if cursor > word.len() || word.len() >= cap { return Err(Error::Limit("inflection insertion".into())); }
            word.insert(cursor, byte);
            if direction == Direction::Right { cursor += 1; }
        } else {
            if direction == Direction::Left { cursor = cursor.checked_sub(1).ok_or_else(|| Error::Malformed("inflection delete before start".into()))?; }
            if word.get(cursor) != Some(&byte) { return Err(Error::Incomplete("inflection deletion does not match source bytes".into())); }
            word.remove(cursor);
        }
    }
    if word.is_empty() { return Err(Error::Incomplete("inflection rule produced an empty key".into())); }
    Ok(word)
}

#[derive(Default)]
struct Node { edges: BTreeMap<u8, usize>, replacements: Vec<Vec<u8>> }
pub struct OldSuffixIndex { nodes: Vec<Node>, encoding: Encoding }
impl OldSuffixIndex {
    pub fn build(index: &Index, pdb: &PalmDatabase, source: &[u8]) -> Result<Self> {
        index.require_tags(&[7])?;
        let mut this = Self { nodes: vec![Node::default()], encoding: index.encoding };
        for row in &index.rows {
            let values = row.tags.get(&7).ok_or_else(|| Error::Unsupported("old inflection row without tag 7".into()))?;
            if values.len() % 2 != 0 { return Err(Error::Malformed("old inflection needs length/offset pairs".into())); }
            for pair in values.chunks_exact(2) {
                let suffix = index.cncx_flat(pdb, source, pair[1], pair[0] as usize)?;
                let mut node = 0;
                for &byte in suffix.iter().rev() {
                    node = match this.nodes[node].edges.get(&byte).copied() {
                        Some(existing) => existing,
                        None => {
                            let next = this.nodes.len();
                            this.nodes.push(Node::default());
                            this.nodes[node].edges.insert(byte, next);
                            next
                        }
                    };
                }
                this.nodes[node].replacements.push(row.label.clone());
            }
        }
        Ok(this)
    }
    pub fn aliases(&self, base: &[u8], max: usize) -> Result<Vec<Alias>> {
        let mut result = Vec::new();
        let mut node = 0;
        for consumed in 0..=base.len() {
            for suffix in &self.nodes[node].replacements {
                if result.len() >= max { return Err(Error::Limit("old inflection expansion".into())); }
                let mut value = base[..base.len() - consumed].to_vec();
                value.extend_from_slice(suffix);
                result.push(Alias { word: self.encoding.decode(&value)?, group: None });
            }
            if consumed == base.len() { break; }
            match self.nodes[node].edges.get(&base[base.len() - 1 - consumed]) {
                Some(&next) => node = next,
                None => break,
            }
        }
        Ok(result)
    }
}

pub fn new_aliases(row: &IndexRow, inflections: &Index, pdb: &PalmDatabase, source: &[u8], encoding: Encoding, max: usize) -> Result<Vec<Alias>> {
    inflections.require_tags(&[5, 26])?;
    let mut result = Vec::new();
    for &group_id in row.tags.get(&42).into_iter().flatten() {
        let group = inflections.rows.get(group_id as usize).ok_or_else(|| Error::Malformed("inflection group out of range".into()))?;
        let names = group.tags.get(&5).ok_or_else(|| Error::Malformed("inflection group names missing".into()))?;
        let rules = group.tags.get(&26).ok_or_else(|| Error::Malformed("inflection rule references missing".into()))?;
        if names.len() != rules.len() { return Err(Error::Incomplete("inflection name/rule cardinality mismatch".into())); }
        for (&name, &rule_id) in names.iter().zip(rules) {
            if result.len() >= max { return Err(Error::Limit("inflection expansion".into())); }
            let rule = inflections.rows.get(rule_id as usize).ok_or_else(|| Error::Malformed("inflection rule id out of range".into()))?;
            let decoded = apply_rule(&row.label, &rule.label, 4096)?;
            result.push(Alias { word: encoding.decode(&decoded)?, group: Some(inflections.cncx_string(pdb, source, name)?) });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_edit_rules() {
        assert_eq!(apply_rule(b"cat", b"\x02s", 100).unwrap(), b"cats");
        assert_eq!(apply_rule(b"do", b"\x01un", 100).unwrap(), b"undo");
        assert_eq!(apply_rule(b"city", b"\x03y\x02sei", 100).unwrap(), b"cities");
        assert_eq!(apply_rule(b"undo", b"\x04un", 100).unwrap(), b"do");
    }
    #[test]
    fn edits_never_guess_or_ignore_mismatches() {
        assert!(apply_rule(b"city", b"\x03x", 100).is_err());
        assert!(apply_rule(b"a", b"\x13", 100).is_err());
        assert!(apply_rule(b"cat", b"\x02s", 3).is_err());
    }
}

use automerge::Change;
use automerge::ChangeHash;
use automerge::ExpandedChange;

pub fn serialize_change_hashes(hashes: &[ChangeHash]) -> anyhow::Result<Vec<u8>> {
    let serialized = serde_json::to_string(hashes)?;
    Ok(serialized.into_bytes())
}

pub fn deserialize_change_hashes(bytes: &[u8]) -> anyhow::Result<Vec<ChangeHash>> {
    let serialized = std::str::from_utf8(bytes)?;
    let hashes: Vec<ChangeHash> = serde_json::from_str(serialized)?;
    Ok(hashes)
}

pub fn serialize_changes(changes: &[Change]) -> anyhow::Result<Vec<u8>> {
    let serialized = serde_json::to_string(
        &changes
            .iter()
            .map(|change| change.decode())
            .collect::<Vec<ExpandedChange>>(),
    )?;
    Ok(serialized.into_bytes())
}

pub fn deserialize_changes(bytes: &[u8]) -> anyhow::Result<Vec<Change>> {
    let serialized = std::str::from_utf8(bytes)?;
    let changes = serde_json::from_str::<Vec<ExpandedChange>>(serialized)?;
    Ok(changes.into_iter().map(ExpandedChange::into).collect())
}

#[cfg(test)]
mod tests {
    use automerge::transaction::Transactable;
    use automerge::AutoCommit;

    use super::*;

    #[test]
    fn test_change_hashes_roundtrip() {
        let change_hashes = vec![ChangeHash([0; 32])];
        let raw = serialize_change_hashes(&change_hashes).unwrap();
        let deserialized_change_hashes = deserialize_change_hashes(&raw);
        assert!(deserialized_change_hashes.is_ok());
        assert_eq!(change_hashes, deserialized_change_hashes.unwrap());
    }

    #[test]
    fn test_change_roundtrip() {
        let mut doc = AutoCommit::new();
        _ = doc.put(automerge::ROOT, "number", 1234);

        let changes: Vec<automerge::Change> = doc
            .get_changes(&[])
            .unwrap()
            .into_iter()
            .map(automerge::Change::clone)
            .collect();

        let raw = serialize_changes(&changes).unwrap();
        let deserialized_changes = deserialize_changes(&raw);
        assert!(deserialized_changes.is_ok());
        assert_eq!(
            changes
                .into_iter()
                .map(|change| change.clone())
                .collect::<Vec<Change>>(),
            deserialized_changes.unwrap()
        );
    }
}

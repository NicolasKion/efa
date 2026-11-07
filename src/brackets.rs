//! The overview's ship class icons ("brackets"). The client maps ship groups to them in code,
//! so the mapping lives here; the textures come from the CDN like item icons.

/// The bracket a ship group shows in the overview, by the texture's base name.
pub fn ship_bracket(group_id: i32) -> Option<&'static str> {
    Some(match group_id {
        // Frigate hulls.
        25 | 324 | 830 | 831 | 834 | 893 | 1022 | 1527 => "frigate",
        237 => "rookie",
        1283 => "miningfrigate",
        // Destroyer hulls.
        420 | 541 | 1305 | 1534 => "destroyer",
        // Cruiser hulls.
        26 | 358 | 832 | 833 | 894 | 906 | 963 | 1972 | 5087 => "cruiser",
        // Battlecruiser hulls.
        419 | 540 | 1201 => "battlecruiser",
        // Battleship hulls.
        27 | 898 | 900 => "battleship",
        // Haulers and industrials.
        28 | 380 | 1202 => "industrial",
        513 | 902 => "freighter",
        463 | 543 => "miningbarge",
        883 | 941 | 4902 => "industrialcommand",
        // Capitals.
        485 | 4594 => "dreadnought",
        547 | 5120 => "carrier",
        659 => "supercarrier",
        1538 => "forceauxiliary",
        30 => "titan",
        29 => "capsule",
        31 => "shuttle",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subclasses_share_their_hull_icon() {
        assert_eq!(ship_bracket(324), Some("frigate"), "assault frigates");
        assert_eq!(ship_bracket(963), Some("cruiser"), "strategic cruisers");
        assert_eq!(ship_bracket(900), Some("battleship"), "marauders");
        assert_eq!(ship_bracket(6), None);
    }
}

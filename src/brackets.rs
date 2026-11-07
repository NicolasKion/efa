//! The overview's ship class icons ("brackets"). The client maps ship groups to them in code,
//! so the mapping lives here; the textures come from the CDN like item icons.

/// The bracket a ship or structure group shows in the overview, by texture file name without
/// the extension. Ships use the 32 pixel version so the icons stay sharp when scaled.
pub fn ship_bracket(group_id: i32) -> Option<&'static str> {
    Some(match group_id {
        // Frigate hulls.
        25 | 324 | 830 | 831 | 834 | 893 | 1022 | 1527 => "frigate_32",
        237 => "rookie_32",
        1283 => "miningfrigate_32",
        // Destroyer hulls.
        420 | 541 | 1305 | 1534 => "destroyer_32",
        // Cruiser hulls.
        26 | 358 | 832 | 833 | 894 | 906 | 963 | 1972 | 5087 => "cruiser_32",
        // Battlecruiser hulls.
        419 | 540 | 1201 => "battlecruiser_32",
        // Battleship hulls.
        27 | 898 | 900 => "battleship_32",
        // Haulers and industrials.
        28 | 380 | 1202 => "industrial_32",
        513 | 902 => "freighter_32",
        463 | 543 => "miningbarge_32",
        883 | 941 | 4902 => "industrialcommand_32",
        // Capitals.
        485 | 4594 => "dreadnought_32",
        547 | 5120 => "carrier_32",
        659 => "supercarrier_32",
        1538 => "forceauxiliary_32",
        30 => "titan_32",
        29 => "capsule_32",
        31 => "shuttle_32",
        // Upwell structures.
        1657 => "citadellarge",
        1404 => "engineeringcomplexlarge",
        1406 => "refinerylarge",
        1408 => "jumpportalarray",
        2016 => "cynosuralsystemjammer",
        2017 => "cynosuralbeacon",
        4744 => "structure",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subclasses_share_their_hull_icon() {
        assert_eq!(ship_bracket(324), Some("frigate_32"), "assault frigates");
        assert_eq!(ship_bracket(963), Some("cruiser_32"), "strategic cruisers");
        assert_eq!(ship_bracket(900), Some("battleship_32"), "marauders");
        assert_eq!(ship_bracket(6), None);
    }
}

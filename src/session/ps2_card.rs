//! A formatted 8 MB PS2 memory card image, as a new card from the shop comes: LRPS2 makes a
//! missing card itself, but unformatted (every byte erased), and some Games hang on one before
//! they draw anything (wLaunchELF reads `mc0:/SYS-CONF/` at boot and deadlocks there; its own
//! source warns of "a pretty nice looking black screen"). Each PS2 Game gets this card instead
//! the first time it starts (FR-NEW-1).
//!
//! The layout is the PS2's own file system as the BIOS formats it: a superblock, one indirect FAT
//! cluster, the FAT, and a root directory holding only `.` and `..`, each 512-byte page followed
//! by 16 spare bytes carrying its ECC.

const PAGE: usize = 512;
const SPARE: usize = 16;
const PAGES_PER_CLUSTER: usize = 2;
const PAGES_PER_BLOCK: usize = 16;
const CLUSTERS: usize = 8192;
const CLUSTER: usize = PAGE * PAGES_PER_CLUSTER;
/// FAT entries per cluster.
const PER_CLUSTER: usize = CLUSTER / 4;
const FAT_CLUSTERS: usize = CLUSTERS / PER_CLUSTER;
/// The indirect FAT cluster: the first one after the superblock's erase block.
const IFC: usize = PAGES_PER_BLOCK / PAGES_PER_CLUSTER;
const FIRST_FAT: usize = IFC + 1;
/// The first allocatable cluster, which holds the root directory.
const ALLOC_OFFSET: usize = FIRST_FAT + FAT_CLUSTERS;
/// Allocatable clusters: all after `ALLOC_OFFSET` but the last two erase blocks, kept for backup.
const ALLOC_END: usize = CLUSTERS - ALLOC_OFFSET - 2 * (PAGES_PER_BLOCK / PAGES_PER_CLUSTER);
const FREE: u32 = 0x7FFF_FFFF;
const END_OF_CHAIN: u32 = 0xFFFF_FFFF;

/// The size of the card image: 8 MB of pages, each with its spare bytes.
pub const SIZE: usize = CLUSTERS * PAGES_PER_CLUSTER * (PAGE + SPARE);

/// A freshly formatted, empty 8 MB card.
pub fn formatted() -> Vec<u8> {
    let mut card = vec![0xFF; SIZE];

    let mut superblock = vec![0xFF; CLUSTER];
    superblock[..40].fill(0);
    superblock[..35].copy_from_slice(b"Sony PS2 Memory Card Format 1.2.0.0");
    for (at, value) in [
        (0x28, PAGE),
        (0x2A, PAGES_PER_CLUSTER),
        (0x2C, PAGES_PER_BLOCK),
        (0x2E, 0xFF00),
    ] {
        superblock[at..at + 2].copy_from_slice(&(value as u16).to_le_bytes());
    }
    let blocks = CLUSTERS * PAGES_PER_CLUSTER / PAGES_PER_BLOCK;
    // Clusters on the card, the allocatable range, the root directory's cluster and the two
    // backup erase blocks.
    let words = [CLUSTERS, ALLOC_OFFSET, ALLOC_END, 0, blocks - 1, blocks - 2];
    put_words(&mut superblock[0x30..], words.map(|w| w as u32));
    superblock[0x48..0x50].fill(0);
    let mut ifc_list = [0; 32];
    ifc_list[0] = IFC as u32;
    put_words(&mut superblock[0x50..], ifc_list);
    put_words(&mut superblock[0xD0..], [END_OF_CHAIN; 32]); // no bad blocks
    superblock[0x150..0x154].copy_from_slice(&[2, 0x52, 0, 0]); // a PS2 card; Sony's flags
    put_cluster(&mut card, 0, &superblock);

    let mut ifc = vec![0; CLUSTER];
    put_words(&mut ifc, (FIRST_FAT..ALLOC_OFFSET).map(|c| c as u32));
    put_cluster(&mut card, IFC, &ifc);

    // The root directory's cluster is taken and ends its chain; the rest are free.
    let fat = (0..FAT_CLUSTERS * PER_CLUSTER).map(|i| match i {
        0 => END_OF_CHAIN,
        i if i < ALLOC_END => FREE,
        _ => END_OF_CHAIN,
    });
    let mut fat_bytes = vec![0; FAT_CLUSTERS * CLUSTER];
    put_words(&mut fat_bytes, fat);
    for (i, cluster) in fat_bytes.chunks(CLUSTER).enumerate() {
        put_cluster(&mut card, FIRST_FAT + i, cluster);
    }

    let mut root = entry(0x8427, 2, b".");
    root.extend(entry(0xA426, 0, b".."));
    put_cluster(&mut card, ALLOC_OFFSET, &root);
    card
}

fn put_words(to: &mut [u8], words: impl IntoIterator<Item = u32>) {
    for (word, at) in words.into_iter().zip(to.as_chunks_mut::<4>().0) {
        *at = word.to_le_bytes();
    }
}

/// A 512-byte directory entry: its mode, its length (entries, for a directory), cluster 0 and
/// `name`, created and modified at 2026-01-01 00:00.
fn entry(mode: u16, length: u32, name: &[u8]) -> Vec<u8> {
    let mut e = vec![0; PAGE];
    e[..2].copy_from_slice(&mode.to_le_bytes());
    e[4..8].copy_from_slice(&length.to_le_bytes());
    let time = [0, 0, 0, 0, 1, 1, 0xEA, 0x07];
    e[8..16].copy_from_slice(&time);
    e[0x18..0x20].copy_from_slice(&time);
    e[0x40..0x40 + name.len()].copy_from_slice(name);
    e
}

fn put_cluster(card: &mut [u8], cluster: usize, data: &[u8]) {
    for (i, page) in data.chunks(PAGE).enumerate() {
        let at = (cluster * PAGES_PER_CLUSTER + i) * (PAGE + SPARE);
        card[at..at + PAGE].copy_from_slice(page);
        let spare = &mut card[at + PAGE..at + PAGE + SPARE];
        spare.fill(0);
        for (chunk, ecc_at) in page.chunks(128).zip(spare.as_chunks_mut::<3>().0) {
            *ecc_at = ecc(chunk);
        }
    }
}

/// The 3-byte Hamming code a PS2 card keeps for each 128 bytes: column parity, then the line
/// parity of the odd bytes' indices, inverted and plain.
fn ecc(chunk: &[u8]) -> [u8; 3] {
    const COLUMN_MASKS: [u8; 7] = [0x55, 0x33, 0x0F, 0x00, 0xAA, 0xCC, 0xF0];
    let odd = |b: u8| b.count_ones() % 2 == 1;
    let (mut column, mut line_0, mut line_1) = (0x77u8, 0x7Fu8, 0x7Fu8);
    for (i, &b) in chunk.iter().enumerate() {
        for (bit, mask) in COLUMN_MASKS.iter().enumerate() {
            column ^= u8::from(odd(b & mask)) << bit;
        }
        if odd(b) {
            line_0 ^= !(i as u8);
            line_1 ^= i as u8;
        }
    }
    [column, line_0 & 0x7F, line_1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(card: &[u8], page: usize) -> &[u8] {
        &card[page * (PAGE + SPARE)..][..PAGE + SPARE]
    }

    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn the_card_is_8_mb_of_pages_with_their_spare_bytes() {
        assert_eq!(formatted().len(), 8_650_752);
    }

    #[test]
    fn the_superblock_says_formatted_with_the_bios_layout() {
        let card = formatted();
        let sb = page(&card, 0);
        assert!(sb.starts_with(b"Sony PS2 Memory Card Format 1.2.0.0\0"));
        assert_eq!(&sb[0x28..0x2E], &[0x00, 0x02, 0x02, 0x00, 0x10, 0x00]);
        // Clusters, allocation offset and end, root cluster, backup blocks: a BIOS-formatted card's.
        let words: Vec<u32> = (0x30..0x48).step_by(4).map(|at| word(sb, at)).collect();
        assert_eq!(words, [8192, 41, 8135, 0, 1023, 1022]);
        assert_eq!(word(sb, 0x50), 8, "the indirect FAT cluster");
        assert_eq!(word(sb, 0xD0), u32::MAX, "no bad blocks");
        assert_eq!(&sb[0x150..0x152], &[2, 0x52]);
    }

    #[test]
    fn the_fat_has_the_root_directory_and_every_other_cluster_free() {
        let card = formatted();
        let ifc = page(&card, 8 * 2);
        assert_eq!(
            (0..32).map(|i| word(ifc, i * 4)).collect::<Vec<_>>(),
            (9..41).collect::<Vec<_>>()
        );
        let fat = page(&card, 9 * 2);
        assert_eq!(word(fat, 0), u32::MAX);
        assert_eq!(word(fat, 4), 0x7FFF_FFFF);
        let root = page(&card, 41 * 2);
        assert_eq!(&root[..2], &0x8427u16.to_le_bytes());
        assert_eq!(word(root, 4), 2, "`.` and `..`");
        assert_eq!(&root[0x40..0x42], b".\0");
        assert_eq!(&page(&card, 41 * 2 + 1)[0x40..0x43], b"..\0");
    }

    #[test]
    fn each_written_page_carries_its_ecc_and_the_rest_stay_erased() {
        let card = formatted();
        let sb = page(&card, 0);
        let spare: Vec<u8> = sb[..PAGE].chunks(128).flat_map(ecc).collect();
        assert_eq!(&sb[PAGE..PAGE + 12], &spare[..]);
        assert_eq!(&sb[PAGE + 12..], &[0; 4]);
        assert!(page(&card, 41 * 2 + 2).iter().all(|&b| b == 0xFF));
    }

    /// A chunk with an even number of set bits in every column and line keeps the base code; one
    /// set bit flips the column parities it is in and the line parities of its index.
    #[test]
    fn the_ecc_follows_the_bits_set() {
        assert_eq!(ecc(&[0; 128]), [0x77, 0x7F, 0x7F]);
        assert_eq!(ecc(&[0xFF; 128]), [0x77, 0x7F, 0x7F]);
        let mut one = [0; 128];
        one[0] = 1;
        assert_eq!(ecc(&one), [0x70, 0x00, 0x7F]);
        let mut one = [0; 128];
        one[5] = 0x80;
        assert_eq!(ecc(&one), [0x77 ^ 0x70, 0x7F ^ 0x7A, 0x7F ^ 0x05]);
    }
}

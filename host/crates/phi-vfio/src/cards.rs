//! More than one card: which is which, and what each one owns.
//!
//! A card is named by an **index**, 0 to [`MAX_CARDS`] - 1. Everything a
//! running card needs on the host is derived from that index and nothing
//! else, so two cards never share a control socket, a host-memory window,
//! a forwarded port, a ring subnet or a hostname:
//!
//! | resource | card 0 | card N |
//! | --- | --- | --- |
//! | control socket | `<runtime>/phictl/control.sock` | `<runtime>/phictl/N/control.sock` |
//! | host-memory window | `/dev/shm/phi-hostmem` | `/dev/shm/phi-hostmem-N` |
//! | SSH forward | `127.0.0.1:2222` | `127.0.0.1:2222+N` |
//! | ring subnet | `10.9.0.1` host, `10.9.0.2` card | `10.9.N.1`, `10.9.N.2` |
//! | hostname | `phi` | `phiN` |
//! | systemd unit | `phi@0.service` | `phi@N.service` |
//!
//! Card 0 keeps the names the single-card stack used, so nothing written
//! against it changes.
//!
//! # Which card is index 0
//!
//! By default the cards are ordered by PCI address, lowest first. That
//! order is not stable: adding a card on the chipset put it at
//! `24:00.0`, below the original card at `2f:00.0`, and the service that
//! boots "the card" booted the new one with the old one's disk. So the
//! order can be pinned in `~/.config/phi/cards`, one card per line, the
//! line order being the index:
//!
//! ```text
//! # BDF            DISK                          HOSTMEM
//! 0000:2f:00.0     /mnt/1TB-NVMe/phi/disk.img    6G
//! 0000:24:00.0     /mnt/1TB-NVMe/phi/disk1.img   6G
//! ```
//!
//! A card listed there but not enumerated keeps its index (it is reported
//! as absent), so pulling card 1 does not turn card 2 into card 1.

use std::fs;
use std::net::Ipv4Addr;
use std::path::PathBuf;

use crate::sysfs::{find_phis, normalize_bdf};
use crate::{Error, Result};

/// The most cards one host can run. Bounded by the subnet and port
/// schemes below, and generous for any machine this will see.
pub const MAX_CARDS: usize = 16;

/// The SSH forward for card 0; card N uses this plus N.
pub const FIRST_PORT: u16 = 2222;

/// The environment variable that selects a card for every tool.
pub const ENV_CARD: &str = "PHI_CARD";

/// One line of the configuration, or one enumerated card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardEntry {
    /// Position in the list: the card index every tool uses.
    pub index: usize,
    /// Canonical PCI address.
    pub bdf: String,
    /// Disk image to serve as `/dev/phiblk0`, when configured.
    pub disk: Option<String>,
    /// Host memory to give the card (a size such as `6G`), when configured.
    pub host_mem: Option<String>,
    /// Whether the device is on the PCI bus right now.
    pub present: bool,
}

/// `$XDG_CONFIG_HOME/phi/cards`, else `~/.config/phi/cards`.
pub fn config_path() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("phi").join("cards");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    PathBuf::from(home).join(".config").join("phi").join("cards")
}

/// Parse the configuration text. Comments start with `#`; fields are
/// whitespace separated: address, then optionally a disk image and a
/// host-memory size.
pub fn parse_config(text: &str) -> Result<Vec<CardEntry>> {
    let mut out = Vec::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let bdf = normalize_bdf(fields.next().unwrap_or(""))
            .map_err(|_| Error::Sysfs(format!("cards file line {}: not a PCI address: {line:?}", lineno + 1)))?;
        if out.iter().any(|c: &CardEntry| c.bdf == bdf) {
            return Err(Error::Sysfs(format!("cards file line {}: {bdf} listed twice", lineno + 1)));
        }
        if out.len() >= MAX_CARDS {
            return Err(Error::Sysfs(format!("cards file lists more than {MAX_CARDS} cards")));
        }
        out.push(CardEntry {
            index: out.len(),
            bdf,
            disk: fields.next().map(str::to_string),
            host_mem: fields.next().map(str::to_string),
            present: false,
        });
    }
    Ok(out)
}

/// Every card this host knows about, in index order.
///
/// The configuration file when it exists, else the enumerated cards by
/// address. Presence is filled in from sysfs either way.
pub fn list() -> Result<Vec<CardEntry>> {
    let enumerated = find_phis()?;
    let mut cards = match fs::read_to_string(config_path()) {
        Ok(text) => parse_config(&text)?,
        Err(_) => enumerated
            .iter()
            .enumerate()
            .map(|(index, bdf)| CardEntry {
                index,
                bdf: bdf.clone(),
                disk: None,
                host_mem: None,
                present: false,
            })
            .collect(),
    };
    if cards.len() > MAX_CARDS {
        return Err(Error::Sysfs(format!(
            "{} cards enumerated; this stack supports {MAX_CARDS}",
            cards.len()
        )));
    }
    for c in &mut cards {
        c.present = enumerated.contains(&c.bdf);
    }
    Ok(cards)
}

/// The card at `index`, which must exist and be enumerated.
pub fn resolve(index: usize) -> Result<CardEntry> {
    check_index(index)?;
    let cards = list()?;
    let card = cards
        .into_iter()
        .find(|c| c.index == index)
        .ok_or_else(|| Error::Sysfs(format!("no card {index}: {} known (phictl cards)", count_known())))?;
    if !card.present {
        return Err(Error::Sysfs(format!("card {index} ({}) is not on the PCI bus", card.bdf)));
    }
    Ok(card)
}

fn count_known() -> usize {
    list().map(|c| c.len()).unwrap_or(0)
}

/// The index of `bdf` in the current list, if it is listed.
pub fn index_of(bdf: &str) -> Result<Option<usize>> {
    let bdf = normalize_bdf(bdf)?;
    Ok(list()?.into_iter().find(|c| c.bdf == bdf).map(|c| c.index))
}

/// `PHI_CARD` from the environment, if set. Malformed is an error rather
/// than a silent card 0.
pub fn index_from_env() -> Result<Option<usize>> {
    match std::env::var(ENV_CARD) {
        Ok(s) if !s.trim().is_empty() => {
            let i: usize = s
                .trim()
                .parse()
                .map_err(|_| Error::Sysfs(format!("{ENV_CARD}={s:?} is not a card index")))?;
            check_index(i)?;
            Ok(Some(i))
        }
        _ => Ok(None),
    }
}

/// Reject an index this stack cannot address.
pub fn check_index(index: usize) -> Result<()> {
    if index >= MAX_CARDS {
        return Err(Error::Sysfs(format!("card index {index} is out of range (0 to {})", MAX_CARDS - 1)));
    }
    Ok(())
}

/// The control socket of card `index` for an unprivileged daemon or
/// client: in the runtime directory (`/tmp/phictl-UID` without one).
pub fn socket_path(index: usize) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        // SAFETY: getuid has no preconditions.
        .unwrap_or_else(|_| PathBuf::from(format!("/tmp/phictl-{}", unsafe { libc::getuid() })));
    let dir = dir.join("phictl");
    if index == 0 {
        dir.join("control.sock")
    } else {
        dir.join(index.to_string()).join("control.sock")
    }
}

/// The control socket of card `index` for a daemon running as root.
pub fn root_socket_path(index: usize) -> PathBuf {
    let dir = PathBuf::from("/run/phictl");
    if index == 0 {
        dir.join("control.sock")
    } else {
        dir.join(index.to_string()).join("control.sock")
    }
}

/// The shared file that is card `index`'s host memory.
pub fn hostmem_path(index: usize) -> PathBuf {
    if index == 0 {
        PathBuf::from("/dev/shm/phi-hostmem")
    } else {
        PathBuf::from(format!("/dev/shm/phi-hostmem-{index}"))
    }
}

/// The loopback port the SSH forwarder of card `index` listens on.
pub fn forward_port(index: usize) -> u16 {
    FIRST_PORT + index as u16
}

/// The host's address on card `index`'s ring network.
pub fn host_ip(index: usize) -> Ipv4Addr {
    Ipv4Addr::new(10, 9, index as u8, 1)
}

/// The card's own address on its ring network, which its init sets from
/// `PHI_CARD_ADDR` on the kernel command line.
pub fn card_ip(index: usize) -> Ipv4Addr {
    Ipv4Addr::new(10, 9, index as u8, 2)
}

/// The card's hostname: `phi`, then `phi1`, `phi2`, ...
pub fn hostname(index: usize) -> String {
    if index == 0 {
        "phi".into()
    } else {
        format!("phi{index}")
    }
}

/// The systemd user unit that runs card `index`.
pub fn unit_name(index: usize) -> String {
    format!("phi@{index}.service")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_config_in_line_order() {
        let text = "# comment\n\n0000:2f:00.0 /x/disk.img 6G\n24:00.0   # bare address, no disk\n";
        let c = parse_config(text).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].index, 0);
        assert_eq!(c[0].bdf, "0000:2f:00.0");
        assert_eq!(c[0].disk.as_deref(), Some("/x/disk.img"));
        assert_eq!(c[0].host_mem.as_deref(), Some("6G"));
        assert_eq!(c[1].index, 1);
        assert_eq!(c[1].bdf, "0000:24:00.0");
        assert_eq!(c[1].disk, None);
    }

    #[test]
    fn refuses_duplicates_junk_and_too_many() {
        assert!(parse_config("2f:00.0\n2f:00.0\n").is_err());
        assert!(parse_config("not-an-address\n").is_err());
        let many: String = (0..17).map(|i| format!("0000:{i:02x}:00.0\n")).collect();
        assert!(parse_config(&many).is_err());
        let sixteen: String = (0..16).map(|i| format!("0000:{i:02x}:00.0\n")).collect();
        assert_eq!(parse_config(&sixteen).unwrap().len(), 16);
    }

    #[test]
    fn card_zero_keeps_the_single_card_names() {
        assert_eq!(hostmem_path(0), PathBuf::from("/dev/shm/phi-hostmem"));
        assert_eq!(hostmem_path(3), PathBuf::from("/dev/shm/phi-hostmem-3"));
        assert_eq!(root_socket_path(0), PathBuf::from("/run/phictl/control.sock"));
        assert_eq!(root_socket_path(2), PathBuf::from("/run/phictl/2/control.sock"));
        assert_eq!(hostname(0), "phi");
        assert_eq!(hostname(15), "phi15");
        assert_eq!(unit_name(0), "phi@0.service");
    }

    #[test]
    fn ports_and_subnets_never_collide_within_the_limit() {
        let ports: std::collections::HashSet<u16> = (0..MAX_CARDS).map(forward_port).collect();
        assert_eq!(ports.len(), MAX_CARDS);
        let nets: std::collections::HashSet<Ipv4Addr> = (0..MAX_CARDS).map(host_ip).collect();
        assert_eq!(nets.len(), MAX_CARDS);
        assert_eq!(host_ip(0), Ipv4Addr::new(10, 9, 0, 1));
        assert_eq!(card_ip(7), Ipv4Addr::new(10, 9, 7, 2));
        assert!(check_index(MAX_CARDS).is_err());
        assert!(check_index(MAX_CARDS - 1).is_ok());
    }
}

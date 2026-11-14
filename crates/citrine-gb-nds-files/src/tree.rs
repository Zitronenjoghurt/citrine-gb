use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ops::Range;

use crate::{FileType, Group, Narc, NdsRom, RomError, Sdat, SoundKind};

pub type NodeId = u32;

const ROOT: NodeId = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Cart(usize),
    Member { narc: NodeId, index: usize },
    Sound { sdat: NodeId, file: usize },
}

#[derive(Debug, Clone)]
pub struct Node {
    name: String,
    lower: String,
    parent: NodeId,
    children: Range<u32>,
    kind: Option<FileType>,
    size: u64,
    source: Option<Source>,
}

impl Node {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn file_type(&self) -> Option<FileType> {
        self.kind
    }

    pub fn is_folder(&self) -> bool {
        self.kind.is_none()
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn source(&self) -> Option<Source> {
        self.source
    }

    pub fn cart_id(&self) -> Option<usize> {
        match self.source {
            Some(Source::Cart(id)) => Some(id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Name,
    Type,
    Size,
    Id,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    pub text: String,
    pub groups: BTreeSet<Group>,
    pub sort: Sort,
    pub descending: bool,
}

impl Query {
    pub fn is_search(&self) -> bool {
        !self.text.trim().is_empty() || !self.groups.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub id: NodeId,
    pub depth: u16,
    pub open: bool,
}

#[derive(Debug, Clone)]
pub struct FileTree {
    nodes: Vec<Node>,
    files: usize,
}

enum Pending {
    Dir(String, Vec<Pending>),
    File(String, usize),
}

impl FileTree {
    pub fn read(rom: &NdsRom) -> Result<Self, RomError> {
        let mut root = Vec::new();
        for (path, id) in rom.named_files() {
            insert(&mut root, path.split('/'), id);
        }
        let mut tree = FileTree {
            nodes: vec![Node {
                name: String::new(),
                lower: String::new(),
                parent: ROOT,
                children: 0..0,
                kind: None,
                size: 0,
                source: None,
            }],
            files: 0,
        };
        tree.place(rom, ROOT, root)?;
        tree.nodes[0].size = tree
            .children(ROOT)
            .map(|id| tree.nodes[id as usize].size)
            .sum();
        tree.files = tree.nodes.iter().filter(|n| n.kind.is_some()).count();
        Ok(tree)
    }

    fn place(
        &mut self,
        rom: &NdsRom,
        parent: NodeId,
        entries: Vec<Pending>,
    ) -> Result<(), RomError> {
        let start = self.nodes.len() as u32;
        let mut pending = Vec::new();
        for entry in entries {
            let (name, kind, size, source, inner) = match entry {
                Pending::Dir(name, inner) => (name, None, 0, None, Some(inner)),
                Pending::File(name, id) => {
                    let bytes = rom.file_by_id(id)?;
                    let kind = FileType::detect(bytes);
                    (
                        name,
                        Some(kind),
                        bytes.len() as u64,
                        Some(Source::Cart(id)),
                        None,
                    )
                }
            };
            self.nodes.push(Node {
                lower: name.to_lowercase(),
                name,
                parent,
                children: 0..0,
                kind,
                size,
                source,
            });
            pending.push(inner);
        }
        self.nodes[parent as usize].children = start..self.nodes.len() as u32;
        for (offset, inner) in pending.into_iter().enumerate() {
            let id = start + offset as u32;
            match inner {
                Some(inner) => {
                    self.place(rom, id, inner)?;
                    self.nodes[id as usize].size =
                        self.children(id).map(|c| self.nodes[c as usize].size).sum();
                }
                None => self.place_members(rom, id),
            }
        }
        Ok(())
    }

    fn place_members(&mut self, rom: &NdsRom, id: NodeId) {
        if self.nodes[id as usize].kind == Some(FileType::Sdat) {
            self.place_sounds(rom, id);
            return;
        }
        if self.nodes[id as usize].kind != Some(FileType::Narc) {
            return;
        }
        let Ok(bytes) = self.bytes(rom, id) else {
            return;
        };
        let Ok(narc) = Narc::parse(bytes) else {
            return;
        };
        let start = self.nodes.len() as u32;
        for index in 0..narc.len() {
            let member = narc.file(index).unwrap_or_default();
            let name = index.to_string();
            self.nodes.push(Node {
                lower: name.clone(),
                name,
                parent: id,
                children: 0..0,
                kind: Some(FileType::detect(member)),
                size: member.len() as u64,
                source: Some(Source::Member { narc: id, index }),
            });
        }
        self.nodes[id as usize].children = start..self.nodes.len() as u32;
        for member in start..self.nodes.len() as u32 {
            self.place_members(rom, member);
        }
    }

    fn place_sounds(&mut self, rom: &NdsRom, id: NodeId) {
        let Ok(bytes) = self.bytes(rom, id) else {
            return;
        };
        let Ok(sdat) = Sdat::parse(bytes) else {
            return;
        };
        let kinds: Vec<SoundKind> = SoundKind::ALL
            .into_iter()
            .filter(|&kind| !sdat.entries(kind).is_empty())
            .collect();
        let start = self.nodes.len() as u32;
        for kind in &kinds {
            let name = kind.label().to_owned();
            self.nodes.push(Node {
                lower: name.to_lowercase(),
                name,
                parent: id,
                children: 0..0,
                kind: None,
                size: 0,
                source: None,
            });
        }
        self.nodes[id as usize].children = start..self.nodes.len() as u32;
        for (offset, kind) in kinds.into_iter().enumerate() {
            let folder = start + offset as u32;
            let first = self.nodes.len() as u32;
            let mut total = 0;
            for entry in sdat.entries(kind) {
                let member = sdat.file(entry.file).unwrap_or_default();
                let name = entry.label();
                total += member.len() as u64;
                self.nodes.push(Node {
                    lower: name.to_lowercase(),
                    name,
                    parent: folder,
                    children: 0..0,
                    kind: Some(FileType::detect(member)),
                    size: member.len() as u64,
                    source: Some(Source::Sound {
                        sdat: id,
                        file: entry.file,
                    }),
                });
            }
            let end = self.nodes.len() as u32;
            let folder = &mut self.nodes[folder as usize];
            folder.children = first..end;
            folder.size = total;
        }
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    pub fn file_count(&self) -> usize {
        self.files
    }

    pub fn children(&self, id: NodeId) -> Range<NodeId> {
        self.nodes[id as usize].children.clone()
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        (id != ROOT).then(|| self.nodes[id as usize].parent)
    }

    pub fn nearest(&self, id: NodeId, kind: FileType) -> Vec<NodeId> {
        let Some(parent) = self.parent(id) else {
            return Vec::new();
        };
        let mut found: Vec<NodeId> = self
            .children(parent)
            .filter(|&other| self.node(other).file_type() == Some(kind))
            .collect();
        found.sort_by_key(|&other| (other.abs_diff(id), other));
        found
    }

    pub fn path(&self, id: NodeId) -> Vec<&str> {
        let mut names = Vec::new();
        let mut at = id;
        while at != ROOT {
            names.push(self.nodes[at as usize].name.as_str());
            at = self.nodes[at as usize].parent;
        }
        names.reverse();
        names
    }

    pub fn find(&self, path: &[&str]) -> Option<NodeId> {
        let mut at = ROOT;
        for name in path {
            at = self
                .children(at)
                .find(|&c| self.nodes[c as usize].name == *name)?;
        }
        Some(at)
    }

    pub fn bytes<'a>(&self, rom: &NdsRom<'a>, id: NodeId) -> Result<&'a [u8], RomError> {
        match self.nodes[id as usize].source {
            Some(Source::Cart(file)) => rom.file_by_id(file),
            Some(Source::Member { narc, index }) => {
                Narc::parse(self.bytes(rom, narc)?)?.file(index)
            }
            Some(Source::Sound { sdat, file }) => Sdat::parse(self.bytes(rom, sdat)?)?.file(file),
            None => Err(RomError::NoSuchFile(self.path(id).join("/"))),
        }
    }

    fn hits(&self, query: &Query) -> Vec<bool> {
        let text = query.text.trim().to_lowercase();
        let mut named = vec![false; self.nodes.len()];
        let mut hits = vec![false; self.nodes.len()];
        for (id, node) in self.nodes.iter().enumerate().skip(1) {
            named[id] =
                text.is_empty() || node.lower.contains(&text) || named[node.parent as usize];
            let grouped = query.groups.is_empty()
                || node
                    .kind
                    .is_some_and(|kind| query.groups.contains(&kind.group()));
            hits[id] = named[id] && grouped && node.kind.is_some();
        }
        hits
    }

    fn kept(&self, query: &Query) -> Vec<bool> {
        let mut kept = self.hits(query);
        for id in (1..self.nodes.len()).rev() {
            if kept[id] {
                kept[self.nodes[id].parent as usize] = true;
            }
        }
        kept
    }

    pub fn count(&self, query: &Query) -> usize {
        self.hits(query).into_iter().filter(|&hit| hit).count()
    }

    pub fn listing(&self, folder: NodeId, query: &Query) -> Vec<NodeId> {
        let mut children: Vec<NodeId> = self.children(folder).collect();
        children.sort_by(|&a, &b| self.compare(a, b, query));
        children
    }

    pub fn found(&self, query: &Query) -> Vec<NodeId> {
        let hits = self.hits(query);
        let mut found: Vec<NodeId> = (1..self.nodes.len() as NodeId)
            .filter(|&id| hits[id as usize])
            .collect();
        found.sort_by(|&a, &b| self.compare(a, b, query));
        found
    }

    pub fn group_counts(&self) -> BTreeMap<Group, usize> {
        let mut counts = BTreeMap::new();
        for kind in self.nodes.iter().filter_map(|node| node.kind) {
            *counts.entry(kind.group()).or_default() += 1;
        }
        counts
    }

    pub fn rows(&self, query: &Query, toggled: &HashSet<NodeId>) -> Vec<Row> {
        let search = query.is_search();
        let kept = search.then(|| self.kept(query));
        let mut rows = Vec::new();
        self.push_rows(ROOT, 0, query, toggled, kept.as_deref(), &mut rows);
        rows
    }

    fn push_rows(
        &self,
        parent: NodeId,
        depth: u16,
        query: &Query,
        toggled: &HashSet<NodeId>,
        kept: Option<&[bool]>,
        rows: &mut Vec<Row>,
    ) {
        let mut children: Vec<NodeId> = self
            .children(parent)
            .filter(|&id| kept.is_none_or(|kept| kept[id as usize]))
            .collect();
        children.sort_by(|&a, &b| self.compare(a, b, query));
        for id in children {
            let node = &self.nodes[id as usize];
            let open = node.has_children() && (kept.is_some() != toggled.contains(&id));
            rows.push(Row { id, depth, open });
            if open {
                self.push_rows(id, depth + 1, query, toggled, kept, rows);
            }
        }
    }

    fn compare(&self, a: NodeId, b: NodeId, query: &Query) -> Ordering {
        let (x, y) = (&self.nodes[a as usize], &self.nodes[b as usize]);
        let folders = y.is_folder().cmp(&x.is_folder());
        if folders != Ordering::Equal {
            return folders;
        }
        let by = match query.sort {
            Sort::Name => Ordering::Equal,
            Sort::Type => x.kind.cmp(&y.kind),
            Sort::Size => x.size.cmp(&y.size),
            Sort::Id => self.number(a).cmp(&self.number(b)),
        };
        let order = by.then_with(|| natural(&x.lower, &y.lower)).then(a.cmp(&b));
        if query.descending {
            order.reverse()
        } else {
            order
        }
    }

    fn number(&self, id: NodeId) -> Option<usize> {
        match self.nodes[id as usize].source {
            Some(Source::Cart(file)) => Some(file),
            Some(Source::Member { index, .. }) => Some(index),
            Some(Source::Sound { file, .. }) => Some(file),
            None => None,
        }
    }
}

fn insert<'p>(entries: &mut Vec<Pending>, mut parts: impl Iterator<Item = &'p str>, id: usize) {
    let Some(name) = parts.next() else {
        return;
    };
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        entries.push(Pending::File(name.to_owned(), id));
        return;
    }
    let found = entries
        .iter()
        .position(|e| matches!(e, Pending::Dir(dir, _) if dir == name));
    let at = found.unwrap_or_else(|| {
        entries.push(Pending::Dir(name.to_owned(), Vec::new()));
        entries.len() - 1
    });
    if let Pending::Dir(_, inner) = &mut entries[at] {
        insert(inner, rest.into_iter(), id);
    }
}

fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let da = a.iter().take_while(|c| c.is_ascii_digit()).count();
                let db = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let (na, nb) = (trim_zeros(&a[..da]), trim_zeros(&b[..db]));
                let order = na.len().cmp(&nb.len()).then(na.cmp(nb));
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[da..];
                b = &b[db..];
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

fn trim_zeros(digits: &[u8]) -> &[u8] {
    let zeros = digits.iter().take_while(|&&c| c == b'0').count();
    &digits[zeros.min(digits.len().saturating_sub(1))..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{narc, nds};

    fn sample() -> Vec<u8> {
        let textures = narc::build(&[
            b"BTX0\xFF\xFE".to_vec(),
            b"BTX0\xFF\xFE\x01\x01".to_vec(),
            vec![7; 3],
        ]);
        let nested = narc::build(&[narc::build(&[b"RGCN\xFF\xFE".to_vec()])]);
        nds::build(
            b"CPUE",
            &[
                ("fielddata/area_map_tex/map_tex_set.narc", textures),
                ("fielddata/build_model/build_model.narc", nested),
                (
                    "data/sound/sound_data.sdat",
                    b"SDAT\xFF\xFE\x00\x01".to_vec(),
                ),
                ("data/intro.txt", b"Hello\n".to_vec()),
                ("data/10.bin", vec![1; 10]),
                ("data/9.bin", vec![1; 9]),
                ("banner.bin", vec![0; 4]),
            ],
        )
    }

    fn names(tree: &FileTree, rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                format!(
                    "{}{}",
                    "  ".repeat(r.depth as usize),
                    tree.node(r.id).name()
                )
            })
            .collect()
    }

    fn open(tree: &FileTree, paths: &[&[&str]]) -> HashSet<NodeId> {
        paths.iter().map(|p| tree.find(p).unwrap()).collect()
    }

    #[test]
    fn folders_come_first_and_numbers_sort_by_value() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let rows = tree.rows(&Query::default(), &open(&tree, &[&["data"]]));
        assert_eq!(
            names(&tree, &rows),
            [
                "data",
                "  sound",
                "  9.bin",
                "  10.bin",
                "  intro.txt",
                "fielddata",
                "banner.bin"
            ]
        );
    }

    #[test]
    fn narcs_open_like_folders_with_members_by_index() {
        let bytes = sample();
        let rom = NdsRom::parse(&bytes).unwrap();
        let tree = FileTree::read(&rom).unwrap();
        let narc = tree
            .find(&["fielddata", "area_map_tex", "map_tex_set.narc"])
            .unwrap();
        assert_eq!(tree.node(narc).file_type(), Some(FileType::Narc));
        let members: Vec<_> = tree
            .children(narc)
            .map(|id| (tree.node(id).name().to_owned(), tree.node(id).file_type()))
            .collect();
        assert_eq!(
            members,
            [
                ("0".into(), Some(FileType::Nsbtx)),
                ("1".into(), Some(FileType::Nsbtx)),
                ("2".into(), Some(FileType::Data))
            ]
        );
        let second = tree
            .find(&["fielddata", "area_map_tex", "map_tex_set.narc", "1"])
            .unwrap();
        assert_eq!(tree.bytes(&rom, second).unwrap(), b"BTX0\xFF\xFE\x01\x01");
        assert_eq!(tree.node(second).size(), 8);
    }

    #[test]
    fn narcs_inside_narcs_open_too() {
        let bytes = sample();
        let rom = NdsRom::parse(&bytes).unwrap();
        let tree = FileTree::read(&rom).unwrap();
        let inner = tree
            .find(&["fielddata", "build_model", "build_model.narc", "0", "0"])
            .unwrap();
        assert_eq!(tree.node(inner).file_type(), Some(FileType::Ncgr));
        assert_eq!(tree.bytes(&rom, inner).unwrap(), b"RGCN\xFF\xFE");
        assert_eq!(
            tree.path(inner),
            ["fielddata", "build_model", "build_model.narc", "0", "0"]
        );
    }

    #[test]
    fn folder_sizes_add_up_their_files() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let data = tree.find(&["data"]).unwrap();
        assert_eq!(tree.node(data).size(), 8 + 6 + 10 + 9);
        assert_eq!(
            tree.node(tree.find(&["data", "9.bin"]).unwrap()).cart_id(),
            Some(5)
        );
    }

    #[test]
    fn search_keeps_matches_and_their_folders_open() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let query = Query {
            text: "SOUND".into(),
            ..Query::default()
        };
        let rows = tree.rows(&query, &HashSet::new());
        assert_eq!(
            names(&tree, &rows),
            ["data", "  sound", "    sound_data.sdat"]
        );
        assert_eq!(tree.count(&query), 1);
    }

    #[test]
    fn a_matching_folder_keeps_everything_inside() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let query = Query {
            text: "map_tex".into(),
            ..Query::default()
        };
        let rows = tree.rows(&query, &HashSet::new());
        assert_eq!(
            names(&tree, &rows),
            [
                "fielddata",
                "  area_map_tex",
                "    map_tex_set.narc",
                "      0",
                "      1",
                "      2"
            ]
        );
        assert_eq!(tree.count(&query), 4);
    }

    #[test]
    fn a_closed_folder_stays_closed_while_searching() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let query = Query {
            text: "map_tex".into(),
            ..Query::default()
        };
        let closed = open(&tree, &[&["fielddata", "area_map_tex"]]);
        let rows = tree.rows(&query, &closed);
        assert_eq!(names(&tree, &rows), ["fielddata", "  area_map_tex"]);
        assert!(!rows[1].open);
    }

    #[test]
    fn filters_keep_only_the_chosen_groups() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let query = Query {
            groups: [Group::Pictures].into(),
            ..Query::default()
        };
        let rows = tree.rows(&query, &HashSet::new());
        assert_eq!(
            names(&tree, &rows),
            [
                "fielddata",
                "  area_map_tex",
                "    map_tex_set.narc",
                "      0",
                "      1",
                "  build_model",
                "    build_model.narc",
                "      0",
                "        0"
            ]
        );
        assert_eq!(tree.count(&query), 3);
    }

    #[test]
    fn sorts_by_size_both_ways() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let opened = open(&tree, &[&["data"]]);
        let mut query = Query {
            sort: Sort::Size,
            ..Query::default()
        };
        let rows = tree.rows(&query, &opened);
        assert_eq!(
            &names(&tree, &rows)[2..5],
            ["  intro.txt", "  9.bin", "  10.bin"]
        );
        query.descending = true;
        let rows = tree.rows(&query, &opened);
        assert_eq!(names(&tree, &rows)[0], "fielddata");
        assert_eq!(names(&tree, &rows).last().unwrap(), "banner.bin");
    }

    #[test]
    fn counts_every_file_and_member() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        assert_eq!(tree.file_count(), 7 + 3 + 2);
        assert_eq!(tree.count(&Query::default()), 12);
    }

    #[test]
    fn a_broken_narc_stays_a_plain_file() {
        let bytes = nds::build(b"CPUE", &[("a.narc", b"NARC\xFE\xFF\x00\x01".to_vec())]);
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let narc = tree.find(&["a.narc"]).unwrap();
        assert_eq!(tree.node(narc).file_type(), Some(FileType::Narc));
        assert!(!tree.node(narc).has_children());
    }

    #[test]
    fn a_folder_lists_its_children_sorted() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let data = tree.find(&["data"]).unwrap();
        let listed: Vec<&str> = tree
            .listing(data, &Query::default())
            .into_iter()
            .map(|id| tree.node(id).name())
            .collect();
        assert_eq!(listed, ["sound", "9.bin", "10.bin", "intro.txt"]);
    }

    #[test]
    fn found_lists_every_hit_without_folders() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let query = Query {
            text: "bin".into(),
            ..Query::default()
        };
        let found: Vec<&str> = tree
            .found(&query)
            .into_iter()
            .map(|id| tree.node(id).name())
            .collect();
        assert_eq!(found, ["9.bin", "10.bin", "banner.bin"]);
    }

    #[test]
    fn counts_files_by_group() {
        let bytes = sample();
        let tree = FileTree::read(&NdsRom::parse(&bytes).unwrap()).unwrap();
        let counts = tree.group_counts();
        assert_eq!(counts[&Group::Pictures], 3);
        assert_eq!(counts[&Group::Archives], 3);
        assert_eq!(counts[&Group::Sound], 1);
        assert_eq!(counts[&Group::Text], 1);
        assert_eq!(counts.values().sum::<usize>(), tree.file_count());
    }

    #[test]
    fn natural_order_reads_numbers_as_numbers() {
        let mut names = vec!["a10", "a9", "a010", "b", "a"];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(names, ["a", "a9", "a10", "a010", "b"]);
    }

    #[test]
    fn nearest_lists_siblings_of_a_kind_closest_first() {
        let bytes = nds::build(
            b"CPUE",
            &[(
                "poketool/pokegra.narc",
                narc::build(&[
                    b"RLCN\xFF\xFE".to_vec(),
                    b"RGCN\xFF\xFE".to_vec(),
                    b"RGCN\xFF\xFE".to_vec(),
                    b"RLCN\xFF\xFE".to_vec(),
                    b"RGCN\xFF\xFE".to_vec(),
                    b"RLCN\xFF\xFE".to_vec(),
                ]),
            )],
        );
        let rom = NdsRom::parse(&bytes).unwrap();
        let tree = FileTree::read(&rom).unwrap();
        let at = |name: &str| tree.find(&["poketool", "pokegra.narc", name]).unwrap();
        let names: Vec<_> = tree
            .nearest(at("2"), FileType::Nclr)
            .into_iter()
            .map(|id| tree.node(id).name())
            .collect();
        assert_eq!(names, ["3", "0", "5"]);
    }

    #[test]
    fn sound_archives_open_as_folders_by_kind_with_their_names() {
        use crate::sdat::{FixtureSound, build};
        let sound = |kind, name, magic: &[u8; 4]| FixtureSound {
            kind,
            name,
            links: [0, 0xFFFF, 0xFFFF, 0xFFFF],
            bytes: [magic.as_slice(), &[0xFF, 0xFE, 0, 1]].concat(),
        };
        let sdat = build(&[
            sound(SoundKind::Sequence, Some("SEQ_TITLE"), b"SSEQ"),
            sound(SoundKind::Bank, Some("BANK_TITLE"), b"SBNK"),
            sound(SoundKind::Stream, None, b"STRM"),
        ]);
        let bytes = nds::build(b"CPUE", &[("sound.sdat", sdat)]);
        let rom = NdsRom::parse(&bytes).unwrap();
        let tree = FileTree::read(&rom).unwrap();
        let archive = tree.find(&["sound.sdat"]).unwrap();
        let folders: Vec<&str> = tree
            .children(archive)
            .map(|id| tree.node(id).name())
            .collect();
        assert_eq!(folders, ["Sequences", "Banks", "Streams"]);
        let sequence = tree
            .find(&["sound.sdat", "Sequences", "SEQ_TITLE"])
            .unwrap();
        assert_eq!(tree.node(sequence).file_type(), Some(FileType::Sseq));
        assert_eq!(&tree.bytes(&rom, sequence).unwrap()[..4], b"SSEQ");
        let stream = tree.find(&["sound.sdat", "Streams", "0"]).unwrap();
        assert_eq!(tree.node(stream).file_type(), Some(FileType::Strm));
        assert_eq!(
            tree.node(tree.find(&["sound.sdat", "Banks"]).unwrap())
                .size(),
            8
        );
    }
}

use std::{
    collections::HashMap,
    ffi::{
        OsStr,
        OsString,
    },
    io::Write,
    path::{
        Path,
        PathBuf,
    },
    rc::Rc,
};

use anyhow::{
    Context,
    Result,
    bail,
};
use crate::fsverity::Sha256HashValue;

#[derive(Debug)]
pub struct Stat {
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_mtim_sec: i64,
    pub xattrs: Vec<(OsString, Vec<u8>)>,
}

#[derive(Debug)]
pub enum LeafContent {
    InlineFile(Vec<u8>),
    ExternalFile(Sha256HashValue, u64),
    BlockDevice(u64),
    CharacterDevice(u64),
    Fifo,
    Socket,
    Symlink(PathBuf),
}

#[derive(Debug)]
pub struct Leaf {
    pub stat: Stat,
    pub content: LeafContent,
}

#[derive(Debug)]
pub struct Directory {
    stat: Stat,
    entries: Vec<DirEnt>
}

#[derive(Debug)]
pub enum Inode {
    Directory(Box<Directory>),
    Leaf(Rc<Leaf>),
}

#[derive(Debug)]
pub struct DirEnt {
    name: OsString,
    inode: Inode,
}

impl Directory {
    pub fn find_entry(&self, name: &OsStr) -> Result<usize, usize> {
        // performance TODO: on the first pass through we'll almost always want the last entry
        // (since the layer is sorted and we're always inserting into the directory that we just
        // created) maybe add a special case for that?
        self.entries.binary_search_by_key(&name, |e| &e.name)
    }

    pub fn recurse<'a>(&'a mut self, name: &OsStr) -> Result<&'a mut Directory> {
        match self.find_entry(name) {
            Ok(idx) => match &mut self.entries[idx].inode {
                Inode::Directory(ref mut subdir) => Ok(subdir),
                _ => bail!("Parent directory is not a directory"),
            },
            _ => bail!("Unable to find parent directory {:?}", name),
        }
    }

    pub fn mkdir(&mut self, name: &OsStr, stat: Stat) {
        match self.find_entry(name) {
            Ok(idx) => match self.entries[idx].inode {
                // Entry already exists, is a dir
                Inode::Directory(ref mut dir) => {
                    // update the stat, but don't drop the entries
                    dir.stat = stat;
                },
                // Entry already exists, is not a dir
                Inode::Leaf(..) => {
                    todo!("Trying to replace non-dir with dir!");
                }
            },
            // Entry doesn't exist yet
            Err(idx) => {
                self.entries.insert(idx, DirEnt {
                    name: OsString::from(name),
                    inode: Inode::Directory(Box::new(Directory {
                        stat, entries: vec![]
                    }))
                });
            }
        }
    }

    pub fn insert(&mut self, name: &OsStr, leaf: Rc<Leaf>) {
        match self.find_entry(name) {
            Ok(idx) => {
                // found existing item
                self.entries[idx].inode = Inode::Leaf(leaf);
            }
            Err(idx) => {
                // need to add new item
                self.entries.insert(idx, DirEnt {
                    name: OsString::from(name),
                    inode: Inode::Leaf(leaf)
                });
            }
        }
    }

    pub fn get_for_link(&self, name: &OsStr) -> Result<Rc<Leaf>> {
        match self.find_entry(name) {
            Ok(idx) => match self.entries[idx].inode {
                Inode::Leaf(ref leaf) => Ok(Rc::clone(leaf)),
                Inode::Directory(..) => bail!("Cannot hardlink to directory"),
            },
            _ => bail!("Attempt to hardlink to non-existent file")
        }
    }

    pub fn remove(&mut self, name: &OsStr) {
        match self.find_entry(name) {
            Ok(idx) => { self.entries.remove(idx); }
            _ => { /* not an error to remove an already-missing file */ }
        }
    }

    pub fn write<W: Write>(&self, writer: &mut W, dirname: &Path, hardlinks: &mut HashMap<*const Leaf, PathBuf>) -> Result<()> {
        writeln!(writer, "{:?} -> dir", dirname)?;
        for DirEnt { name, inode } in self.entries.iter() {
            let path = dirname.join(name);

            match inode {
                Inode::Directory(dir) => dir.write(writer, &path, hardlinks)?,
                Inode::Leaf(leaf) => {
                    if Rc::strong_count(leaf) > 1 {
                        let ptr = Rc::as_ptr(leaf);
                        if let Some(target) = hardlinks.get(&ptr) {
                            writeln!(writer, "{:?} -> hard {:?}", name, target)?;
                        } else {
                            writeln!(writer, "{:?} -> hard.", name)?;
                            hardlinks.insert(ptr, path);
                        }
                    } else {
                        writeln!(writer, "{:?} -> file", path)?;
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct FileSystem {
    root: Directory
}

impl FileSystem {
    pub fn new() -> Self {
        FileSystem {
            root: Directory {
                stat: Stat { st_mode: 0o755, st_uid: 0, st_gid: 0, st_mtim_sec: 0, xattrs: vec![], },
                entries: vec![],
            }
        }
    }

    fn get_parent_dir<'a>(&'a mut self, name: &Path) -> Result<&'a mut Directory> {
        let mut dir = &mut self.root;

        if let Some(parent) = name.parent() {
            for segment in parent {
                if segment.is_empty() || segment == "/" { // Path.parent() is really weird...
                    continue;
                }
                dir = dir.recurse(segment)
                    .with_context(|| format!("Trying to insert item {:?}", name))?;
            }
        }

        Ok(dir)
    }

    pub fn mkdir(&mut self, name: &Path, stat: Stat) -> Result<()> {
        if let Some(filename) = name.file_name() {
            let dir = self.get_parent_dir(name)?;
            dir.mkdir(filename, stat);
        }
        Ok(())
    }

    pub fn insert_rc(&mut self, name: &Path, leaf: Rc<Leaf>) -> Result<()> {
        if let Some(filename) = name.file_name() {
            let dir = self.get_parent_dir(name)?;
            dir.insert(filename, leaf);
            Ok(())
        } else {
            todo!()
        }
    }

    pub fn insert(&mut self, name: &Path, leaf: Leaf) -> Result<()> {
        self.insert_rc(name, Rc::new(leaf))
    }

    fn get_for_link(&mut self, name: &Path) -> Result<Rc<Leaf>> {
        if let Some(filename) = name.file_name() {
            let dir = self.get_parent_dir(name)?;
            dir.get_for_link(filename)
        } else {
            todo!()
        }
    }

    pub fn hardlink(&mut self, name: &Path, target: &Path) -> Result<()> {
        let rc = self.get_for_link(target)?;
        self.insert_rc(name, rc)
    }

    pub fn remove(&mut self, name: &Path) -> Result<()> {
        if let Some(filename) = name.file_name() {
            let dir = self.get_parent_dir(name)?;
            dir.remove(filename);
            Ok(())
        } else {
            todo!();
        }
    }

    pub fn dump<W: Write>(&self, writer: &mut W) -> Result<()> {
        let mut hardlinks = HashMap::new();
        self.root.write(writer, Path::new("/"), &mut hardlinks)?;
        Ok(())
    }
}

//! What the core needs from the system that differs in a browser: the wall
//! clock, background work and files. Desktop and Android use the standard
//! library; WebAssembly in a page uses the page's clock, runs work inline and
//! keeps files in memory, which the page saves to IndexedDB (see
//! [`fs::take_changes`]).

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
    }
}

/// Runs `work` on a new thread, or right away where there are no threads.
pub fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        work();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = std::thread::Builder::new().name(name.into()).spawn(work);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use std::fs;

/// Files kept in memory, with the same calls as the parts of `std::fs` the
/// app uses. Directories exist implicitly.
#[cfg(target_arch = "wasm32")]
pub mod fs {
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::{self, Read, Write};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static FILES: Mutex<BTreeMap<PathBuf, Vec<u8>>> = Mutex::new(BTreeMap::new());
    /// Paths written or removed since the page last saved.
    static CHANGED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

    fn not_found() -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, "No such file")
    }

    fn put(path: &Path, bytes: Vec<u8>) {
        FILES.lock().unwrap().insert(path.to_path_buf(), bytes);
        CHANGED.lock().unwrap().insert(path.to_path_buf());
    }

    /// Restores a file the page saved earlier, before the app starts.
    pub fn preload(path: &str, bytes: Vec<u8>) {
        FILES.lock().unwrap().insert(PathBuf::from(path), bytes);
    }

    /// Files changed since the last call: contents, or `None` when removed.
    pub fn take_changes() -> Vec<(String, Option<Vec<u8>>)> {
        let changed = std::mem::take(&mut *CHANGED.lock().unwrap());
        let files = FILES.lock().unwrap();
        changed.into_iter().map(|p| (p.to_string_lossy().into_owned(), files.get(&p).cloned())).collect()
    }

    pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        FILES.lock().unwrap().get(path.as_ref()).cloned().ok_or_else(not_found)
    }

    pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
        String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        put(path.as_ref(), bytes.as_ref().to_vec());
        Ok(())
    }

    pub fn create_dir_all(_: impl AsRef<Path>) -> io::Result<()> {
        Ok(())
    }

    pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
        let bytes = FILES.lock().unwrap().remove(from.as_ref()).ok_or_else(not_found)?;
        CHANGED.lock().unwrap().insert(from.as_ref().to_path_buf());
        put(to.as_ref(), bytes);
        Ok(())
    }

    pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
        FILES.lock().unwrap().remove(path.as_ref()).ok_or_else(not_found)?;
        CHANGED.lock().unwrap().insert(path.as_ref().to_path_buf());
        Ok(())
    }

    pub fn remove_dir_all(dir: impl AsRef<Path>) -> io::Result<()> {
        let mut files = FILES.lock().unwrap();
        let gone: Vec<PathBuf> = files.keys().filter(|p| p.starts_with(dir.as_ref())).cloned().collect();
        let mut changed = CHANGED.lock().unwrap();
        for p in gone {
            files.remove(&p);
            changed.insert(p);
        }
        Ok(())
    }

    pub struct Metadata {
        len: u64,
    }

    impl Metadata {
        pub fn is_file(&self) -> bool {
            true
        }
        pub fn len(&self) -> u64 {
            self.len
        }
    }

    pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
        FILES.lock().unwrap().get(path.as_ref()).map(|b| Metadata { len: b.len() as u64 }).ok_or_else(not_found)
    }

    pub struct DirEntry(PathBuf);

    impl DirEntry {
        pub fn path(&self) -> PathBuf {
            self.0.clone()
        }
    }

    /// Files and folders directly inside `dir`.
    pub fn read_dir(dir: impl AsRef<Path>) -> io::Result<std::vec::IntoIter<io::Result<DirEntry>>> {
        let dir = dir.as_ref();
        let files = FILES.lock().unwrap();
        let mut seen = BTreeSet::new();
        for p in files.keys() {
            if let Ok(rest) = p.strip_prefix(dir) {
                if let Some(first) = rest.components().next() {
                    seen.insert(dir.join(first));
                }
            }
        }
        Ok(seen.into_iter().map(|p| Ok(DirEntry(p))).collect::<Vec<_>>().into_iter())
    }

    /// An open file. Writes land when it is synced or dropped.
    pub struct File {
        path: PathBuf,
        data: Vec<u8>,
        pos: usize,
        writing: bool,
    }

    impl File {
        pub fn create(path: impl AsRef<Path>) -> io::Result<File> {
            Ok(File { path: path.as_ref().to_path_buf(), data: Vec::new(), pos: 0, writing: true })
        }

        pub fn create_new(path: impl AsRef<Path>) -> io::Result<File> {
            if FILES.lock().unwrap().contains_key(path.as_ref()) {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "File exists"));
            }
            File::create(path)
        }

        pub fn open(path: impl AsRef<Path>) -> io::Result<File> {
            Ok(File { path: path.as_ref().to_path_buf(), data: read(path.as_ref())?, pos: 0, writing: false })
        }

        pub fn sync_all(&self) -> io::Result<()> {
            if self.writing {
                put(&self.path, self.data.clone());
            }
            Ok(())
        }
    }

    impl Drop for File {
        fn drop(&mut self) {
            let _ = self.sync_all();
        }
    }

    impl Write for File {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.data.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Read for File {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.data.len() - self.pos);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }
}

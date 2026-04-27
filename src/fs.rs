use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use chrono::{DateTime, Utc};
use crate::error::ZillError;
use path_clean::PathClean;

/// Metadata for a file in the VirtualFs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub size: usize,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub content: Vec<u8>,
}

/// Metadata for a directory in the VirtualFs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirMeta {
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub children: HashSet<String>,
}

/// A node in the VirtualFs tree, either a file or a directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    File(FileMeta),
    Directory(DirMeta),
}

impl Node {
    /// Returns true if the node is a directory.
    pub fn is_dir(&self) -> bool {
        matches!(self, Node::Directory(_))
    }

    /// Returns true if the node is a file.
    pub fn is_file(&self) -> bool {
        matches!(self, Node::File(_))
    }
}

/// An in-memory virtual file system.
///
/// Note: The derived Serialize and Deserialize implementations use a flat HashMap format,
/// which is different from the human-readable nested JSON format produced by
/// `ZillSession::to_json` and `ZillSession::from_json`. Use those methods for
/// agent-facing serialization.
#[derive(Serialize, Deserialize)]
pub struct VirtualFs {
    pub nodes: HashMap<PathBuf, Node>,
    pub max_nodes: usize,
    pub max_file_size: usize,
}

/// A node representation used for nested JSON serialization.
#[derive(Serialize, Deserialize)]
pub struct NestedNode {
    pub name: String,
    pub node: Node,
    pub children: Option<Vec<NestedNode>>,
}

impl VirtualFs {
    /// Creates a new empty VirtualFs with the specified resource limits.
    pub fn new(max_nodes: usize, max_file_size: usize) -> Self {
        let mut nodes = HashMap::new();
        let now = Utc::now();
        nodes.insert(
            PathBuf::from("/"),
            Node::Directory(DirMeta {
                created_at: now,
                modified_at: now,
                children: HashSet::new(),
            }),
        );
        VirtualFs {
            nodes,
            max_nodes,
            max_file_size,
        }
    }

    /// Canonicalizes a path relative to the current working directory.
    pub fn canonicalize(&self, path: &Path, cwd: &Path) -> PathBuf {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            cwd.join(path)
        };
        let mut cleaned = absolute.clean();
        if !cleaned.starts_with("/") {
             cleaned = PathBuf::from("/").join(cleaned).clean();
        }
        cleaned
    }

    /// Returns the metadata for the node at the specified path.
    pub fn stat(&self, path: &Path) -> Result<&Node, ZillError> {
        self.nodes.get(path).ok_or_else(|| ZillError::NotFound(path.display().to_string()))
    }

    /// Recursively creates directories for the specified path.
    pub fn mkdir_p(&mut self, path: &Path) -> Result<(), ZillError> {
        let mut current = PathBuf::from("/");
        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                let next = current.join(name);
                let name_str = name.to_string_lossy().into_owned();

                if !self.nodes.contains_key(&next) {
                    if self.nodes.len() >= self.max_nodes {
                        return Err(ZillError::DiskFull);
                    }

                    // Update parent
                    if let Some(Node::Directory(ref mut meta)) = self.nodes.get_mut(&current) {
                        meta.children.insert(name_str.clone());
                        meta.modified_at = Utc::now();
                    }

                    let now = Utc::now();
                    self.nodes.insert(
                        next.clone(),
                        Node::Directory(DirMeta {
                            created_at: now,
                            modified_at: now,
                            children: HashSet::new(),
                        }),
                    );
                } else {
                    let node = self.nodes.get(&next).unwrap();
                    if !node.is_dir() {
                        return Err(ZillError::NotADirectory(next.display().to_string()));
                    }
                }
                current = next;
            }
        }
        Ok(())
    }

    /// Creates a new file at the specified path with the given content.
    pub fn create_file(&mut self, path: &Path, content: Vec<u8>) -> Result<(), ZillError> {
        if content.len() > self.max_file_size {
            return Err(ZillError::FileTooLarge);
        }

        let parent = path.parent().ok_or_else(|| ZillError::InvalidPath("No parent".into()))?;
        self.mkdir_p(parent)?;

        let filename = path.file_name().ok_or_else(|| ZillError::InvalidPath("No filename".into()))?;
        let filename_str = filename.to_string_lossy().into_owned();

        if let Some(node) = self.nodes.get(path) {
            if node.is_dir() {
                return Err(ZillError::IsADirectory(path.display().to_string()));
            }
        } else if self.nodes.len() >= self.max_nodes {
            return Err(ZillError::DiskFull);
        }

        let now = Utc::now();
        let file_meta = FileMeta {
            size: content.len(),
            created_at: now,
            modified_at: now,
            content,
        };

        self.nodes.insert(path.to_path_buf(), Node::File(file_meta));

        // Update parent
        if let Some(Node::Directory(ref mut meta)) = self.nodes.get_mut(parent) {
            meta.children.insert(filename_str);
            meta.modified_at = now;
        }

        Ok(())
    }

    /// Reads the content of the file at the specified path.
    pub fn read(&self, path: &Path) -> Result<&[u8], ZillError> {
        match self.stat(path)? {
            Node::File(meta) => Ok(&meta.content),
            Node::Directory(_) => Err(ZillError::IsADirectory(path.display().to_string())),
        }
    }

    /// Writes content to a file at the specified path, creating it if it doesn't exist.
    pub fn write(&mut self, path: &Path, content: Vec<u8>) -> Result<(), ZillError> {
        if content.len() > self.max_file_size {
            return Err(ZillError::FileTooLarge);
        }

        match self.nodes.get_mut(path) {
            Some(Node::File(ref mut meta)) => {
                meta.size = content.len();
                meta.content = content;
                meta.modified_at = Utc::now();
                Ok(())
            }
            Some(Node::Directory(_)) => Err(ZillError::IsADirectory(path.display().to_string())),
            None => self.create_file(path, content),
        }
    }

    /// Returns a sorted list of entry names in the specified directory.
    pub fn list_dir(&self, path: &Path) -> Result<Vec<String>, ZillError> {
        match self.stat(path)? {
            Node::Directory(meta) => {
                let mut children: Vec<String> = meta.children.iter().cloned().collect();
                children.sort();
                Ok(children)
            }
            Node::File(_) => Err(ZillError::NotADirectory(path.display().to_string())),
        }
    }

    /// Removes a file or an empty directory at the specified path.
    pub fn remove(&mut self, path: &Path) -> Result<(), ZillError> {
        if path == Path::new("/") {
            return Err(ZillError::PermissionDenied("Cannot remove root".into()));
        }

        let node = self.nodes.get(path).ok_or_else(|| ZillError::NotFound(path.display().to_string()))?;
        if let Node::Directory(meta) = node {
            if !meta.children.is_empty() {
                return Err(ZillError::DirectoryNotEmpty(path.display().to_string()));
            }
        }

        self.nodes.remove(path);

        let parent = path.parent().unwrap();
        let filename = path.file_name().unwrap().to_string_lossy();
        if let Some(Node::Directory(ref mut meta)) = self.nodes.get_mut(parent) {
            meta.children.remove(filename.as_ref());
            meta.modified_at = Utc::now();
        }

        Ok(())
    }

    /// Serializes the VirtualFs into a nested tree structure for better readability.
    pub fn serialize_nested<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct NestedVfs {
            nodes: NestedNode,
            max_nodes: usize,
            max_file_size: usize,
        }

        let root = self.get_nested_node(Path::new("/"), "/").map_err(serde::ser::Error::custom)?;
        let nested = NestedVfs {
            nodes: root,
            max_nodes: self.max_nodes,
            max_file_size: self.max_file_size,
        };
        nested.serialize(serializer)
    }

    /// Deserializes the VirtualFs from a nested tree structure.
    pub fn deserialize_nested<'de, D>(deserializer: D) -> Result<VirtualFs, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct NestedVfs {
            nodes: NestedNode,
            max_nodes: usize,
            max_file_size: usize,
        }

        let nested: NestedVfs = Deserialize::deserialize(deserializer)?;
        let mut nodes = HashMap::new();
        let mut node_count = 0;
        Self::flatten_nested_node(
            Path::new("/"),
            nested.nodes,
            &mut nodes,
            &mut node_count,
            nested.max_nodes,
            nested.max_file_size
        ).map_err(serde::de::Error::custom)?;

        Ok(VirtualFs {
            nodes,
            max_nodes: nested.max_nodes,
            max_file_size: nested.max_file_size,
        })
    }

    fn flatten_nested_node(
        path: &Path,
        nested: NestedNode,
        nodes: &mut HashMap<PathBuf, Node>,
        node_count: &mut usize,
        max_nodes: usize,
        max_file_size: usize,
    ) -> Result<(), String> {
        *node_count += 1;
        if *node_count > max_nodes {
            return Err("max nodes exceeded during deserialization".to_string());
        }

        let mut node = nested.node;
        if let Node::File(ref meta) = node {
            if meta.content.len() > max_file_size {
                return Err(format!("file {} exceeds max file size during deserialization", path.display()));
            }
        }

        if let Some(children) = nested.children {
            if let Node::Directory(ref mut meta) = node {
                let mut children_set = HashSet::new();
                for child in children {
                    if child.name.is_empty() || child.name == "." || child.name == ".." || child.name.contains('/') {
                        return Err(format!("invalid child name '{}' at {}", child.name, path.display()));
                    }
                    if !children_set.insert(child.name.clone()) {
                        return Err(format!("duplicate child name '{}' at {}", child.name, path.display()));
                    }
                    let child_path = path.join(&child.name);
                    Self::flatten_nested_node(&child_path, child, nodes, node_count, max_nodes, max_file_size)?;
                }
                meta.children = children_set;
            } else {
                return Err(format!("node at {} has children but is not a directory", path.display()));
            }
        } else if let Node::Directory(ref mut meta) = node {
            meta.children.clear();
        }

        nodes.insert(path.to_path_buf(), node);
        Ok(())
    }

    fn get_nested_node(&self, path: &Path, name: &str) -> Result<NestedNode, ZillError> {
        let node = self.stat(path)?.clone();
        let mut children = None;
        if let Node::Directory(ref meta) = node {
            let mut nested_children = Vec::new();
            for child_name in &meta.children {
                nested_children.push(self.get_nested_node(&path.join(child_name), child_name)?);
            }
            nested_children.sort_by(|a, b| a.name.cmp(&b.name));
            children = Some(nested_children);
        }
        Ok(NestedNode {
            name: name.to_string(),
            node,
            children,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_vfs_basic() {
        let mut fs = VirtualFs::new(100, 1024);
        let root_path = Path::new("/");

        // Check root exists
        assert!(fs.stat(root_path).is_ok());

        // Create file
        let file_path = Path::new("/test.txt");
        fs.create_file(file_path, b"hello".to_vec()).unwrap();

        // Read file
        assert_eq!(fs.read(file_path).unwrap(), b"hello");

        // List dir
        let children = fs.list_dir(root_path).unwrap();
        assert_eq!(children, vec!["test.txt".to_string()]);
    }

    #[test]
    fn test_mkdir_p() {
        let mut fs = VirtualFs::new(100, 1024);
        fs.mkdir_p(Path::new("/a/b/c")).unwrap();

        assert!(fs.stat(Path::new("/a")).unwrap().is_dir());
        assert!(fs.stat(Path::new("/a/b")).unwrap().is_dir());
        assert!(fs.stat(Path::new("/a/b/c")).unwrap().is_dir());

        let children = fs.list_dir(Path::new("/a/b")).unwrap();
        assert_eq!(children, vec!["c".to_string()]);
    }

    #[test]
    fn test_canonicalize() {
        let fs = VirtualFs::new(100, 1024);
        let cwd = Path::new("/home/user");

        assert_eq!(fs.canonicalize(Path::new("file.txt"), cwd), PathBuf::from("/home/user/file.txt"));
        assert_eq!(fs.canonicalize(Path::new("../other"), cwd), PathBuf::from("/home/other"));
        assert_eq!(fs.canonicalize(Path::new("/abs/path"), cwd), PathBuf::from("/abs/path"));
        assert_eq!(fs.canonicalize(Path::new("../../../.."), cwd), PathBuf::from("/"));
    }

    #[test]
    fn test_limits() {
        let mut fs = VirtualFs::new(2, 10); // Root is 1 node

        // This should work (adds /test.txt)
        assert!(fs.create_file(Path::new("/test.txt"), b"1234567890".to_vec()).is_ok());

        // This should fail (exceeds max_nodes)
        assert!(fs.create_file(Path::new("/other.txt"), b"small".to_vec()).is_err());

        // This should fail (exceeds max_file_size)
        let mut big_fs = VirtualFs::new(100, 5);
        assert!(big_fs.create_file(Path::new("/big.txt"), b"123456".to_vec()).is_err());
    }
}

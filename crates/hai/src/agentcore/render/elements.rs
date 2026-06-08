use std::fmt::{Display, Formatter};

use indexmap::IndexMap;
use serde::Serialize;

// ─── AttrValue ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AttrValue {
    Null,
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl Serialize for AttrValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            AttrValue::Null => serializer.serialize_unit(),
            AttrValue::String(s) => serializer.serialize_str(s),
            AttrValue::Int(i) => serializer.serialize_i64(*i),
            AttrValue::Float(f) => serializer.serialize_f64(*f),
            AttrValue::Bool(b) => serializer.serialize_bool(*b),
        }
    }
}

impl Display for AttrValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AttrValue::Null => write!(f, "null"),
            AttrValue::String(s) => write!(f, "{}", s),
            AttrValue::Int(i) => write!(f, "{}", i),
            AttrValue::Float(fl) => write!(f, "{}", fl),
            AttrValue::Bool(b) => write!(f, "{}", b),
        }
    }
}

impl From<String> for AttrValue {
    fn from(s: String) -> Self {
        AttrValue::String(s)
    }
}

impl From<&String> for AttrValue {
    fn from(s: &String) -> Self {
        AttrValue::String(s.clone())
    }
}

impl From<&str> for AttrValue {
    fn from(s: &str) -> Self {
        AttrValue::String(s.to_string())
    }
}

impl From<i64> for AttrValue {
    fn from(i: i64) -> Self {
        AttrValue::Int(i)
    }
}

impl From<i32> for AttrValue {
    fn from(i: i32) -> Self {
        AttrValue::Int(i as i64)
    }
}

impl From<f64> for AttrValue {
    fn from(f: f64) -> Self {
        AttrValue::Float(f)
    }
}

impl From<bool> for AttrValue {
    fn from(b: bool) -> Self {
        AttrValue::Bool(b)
    }
}

impl From<uuid::Uuid> for AttrValue {
    fn from(u: uuid::Uuid) -> Self {
        AttrValue::String(u.to_string())
    }
}

impl From<jiff::Timestamp> for AttrValue {
    fn from(t: jiff::Timestamp) -> Self {
        AttrValue::String(t.to_string())
    }
}

// ─── Format ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Format {
    #[default]
    Xml,
    Json,
    Md,
}

// ─── Node ──────────────────────────────────────────────────────────────────────

/// 渲染节点。Elem 有标签、属性、子节点；Text 是纯文本。
#[derive(Debug, Clone)]
pub enum Node {
    Elem {
        tag: String,
        attrs: IndexMap<String, AttrValue>,
        children: Vec<Node>,
    },
    Text(String),
}

impl Node {
    pub fn tag(name: impl Into<String>) -> Self {
        Node::Elem {
            tag: name.into(),
            attrs: IndexMap::new(),
            children: Vec::new(),
        }
    }

    pub fn text(content: impl Into<String>) -> Self {
        Node::Text(content.into())
    }

    pub fn attr(mut self, key: impl Into<String>, value: impl Into<AttrValue>) -> Self {
        if let Node::Elem { ref mut attrs, .. } = self {
            attrs.insert(key.into(), value.into());
        }
        self
    }

    pub fn child(mut self, child: impl Into<Node>) -> Self {
        if let Node::Elem {
            ref mut children, ..
        } = self
        {
            children.push(child.into());
        }
        self
    }

    pub fn children(self, items: Vec<Node>) -> Self {
        match self {
            Node::Elem {
                tag,
                attrs,
                mut children,
            } => {
                children.extend(items);
                Node::Elem {
                    tag,
                    attrs,
                    children,
                }
            }
            Node::Text(_) => self,
        }
    }

    pub fn push_child(&mut self, child: impl Into<Node>) {
        if let Node::Elem { children, .. } = self {
            children.push(child.into());
        }
    }

    pub fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Node::Elem { children, .. } => Some(children),
            Node::Text(_) => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Node::Elem { children, .. } => children.is_empty(),
            Node::Text(_) => false,
        }
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        if let Node::Elem {
            ref mut children, ..
        } = self
        {
            children.push(Node::Text(text.into()));
        }
        self
    }
}

impl From<String> for Node {
    fn from(s: String) -> Self {
        Node::Text(s)
    }
}

impl From<&str> for Node {
    fn from(s: &str) -> Self {
        Node::Text(s.to_string())
    }
}

// ─── Node → serde_json::Value ─────────────────────────────────────────────────

impl Node {
    /// 转换为 serde_json::Value（Item 风格的表示：`_tag` + attrs + children）
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            Node::Text(t) => serde_json::Value::String(t.clone()),
            Node::Elem {
                tag,
                attrs,
                children,
            } => {
                let mut map = serde_json::Map::new();
                map.insert("_tag".to_string(), serde_json::Value::String(tag.clone()));
                for (k, v) in attrs {
                    map.insert(k.clone(), serde_json::to_value(v).unwrap_or_default());
                }
                if !children.is_empty() {
                    map.insert(
                        "children".to_string(),
                        serde_json::Value::Array(
                            children.iter().map(|c| c.to_json_value()).collect(),
                        ),
                    );
                }
                serde_json::Value::Object(map)
            }
        }
    }
}

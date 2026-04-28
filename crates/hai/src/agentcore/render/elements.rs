use std::fmt::{Display, Formatter};

use indexmap::IndexMap;
use jiff::Timestamp;
use serde::Serialize;
use uuid::Uuid;

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
        AttrValue::String(s.into())
    }
}

impl From<&str> for AttrValue {
    fn from(s: &str) -> Self {
        AttrValue::String(s.into())
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

impl From<f32> for AttrValue {
    fn from(f: f32) -> Self {
        AttrValue::Float(f as f64)
    }
}

impl From<bool> for AttrValue {
    fn from(b: bool) -> Self {
        AttrValue::Bool(b)
    }
}

impl From<Timestamp> for AttrValue {
    fn from(ts: Timestamp) -> Self {
        AttrValue::String(ts.to_string())
    }
}

impl From<Uuid> for AttrValue {
    fn from(id: Uuid) -> Self {
        AttrValue::String(id.to_string())
    }
}

impl<T: Into<AttrValue>> From<Option<T>> for AttrValue {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => v.into(),
            None => AttrValue::Null,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum Format {
    #[default]
    Xml,
    Json,
    Md,
}

#[derive(Debug, Clone)]
pub enum RenderElement {
    Section(Section),
    Item(Item),
    Text(Text),
    KeyValue(KeyValue),
    Empty,
}

impl<K, V> From<(K, V)> for KeyValue
where
    K: Into<String>,
    V: Into<String>,
{
    fn from((key, value): (K, V)) -> Self {
        KeyValue::new(key, value)
    }
}

impl<K, V> From<(K, V)> for RenderElement
where
    K: Into<String>,
    V: Into<String>,
{
    fn from((key, value): (K, V)) -> Self {
        RenderElement::KeyValue(KeyValue::new(key, value))
    }
}

impl From<String> for RenderElement {
    fn from(s: String) -> Self {
        RenderElement::Text(Text::new(s))
    }
}

impl From<&str> for RenderElement {
    fn from(s: &str) -> Self {
        RenderElement::Text(Text::new(s))
    }
}

impl<T: Into<String>> From<T> for Text {
    fn from(s: T) -> Self {
        Text::new(s)
    }
}

impl<T: Into<String>> From<T> for Item {
    fn from(tag: T) -> Self {
        Item::new(tag)
    }
}

impl<T: Into<String>> From<T> for Section {
    fn from(tag: T) -> Self {
        Section::new(tag)
    }
}

impl From<Section> for RenderElement {
    fn from(s: Section) -> Self {
        RenderElement::Section(s)
    }
}

impl From<Item> for RenderElement {
    fn from(i: Item) -> Self {
        RenderElement::Item(i)
    }
}

impl From<Text> for RenderElement {
    fn from(t: Text) -> Self {
        RenderElement::Text(t)
    }
}

impl From<KeyValue> for RenderElement {
    fn from(kv: KeyValue) -> Self {
        RenderElement::KeyValue(kv)
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub tag: String,
    pub attrs: IndexMap<String, AttrValue>,
    pub children: Vec<RenderElement>,
}

impl Section {
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    pub fn len(&self) -> usize {
        self.children.len()
    }
}

impl Section {
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            attrs: IndexMap::new(),
            children: Vec::new(),
        }
    }

    pub fn with_attr<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<AttrValue>,
    {
        self.attrs.insert(key.into(), value.into());
        self
    }

    pub fn maybe_with_attr<K, V>(self, key: K, value: Option<V>) -> Self
    where
        K: Into<String>,
        V: Into<AttrValue>,
    {
        if let Some(v) = value {
            self.with_attr(key, v)
        } else {
            self
        }
    }

    pub fn add_child(mut self, child: impl Into<RenderElement>) -> Self {
        self.children.push(child.into());
        self
    }

    pub fn push_child(&mut self, child: impl Into<RenderElement>) {
        self.children.push(child.into());
    }

    pub fn children_mut(&mut self) -> &mut Vec<RenderElement> {
        &mut self.children
    }

    pub fn add_children(
        mut self,
        children: impl IntoIterator<Item = impl Into<RenderElement>>,
    ) -> Self {
        self.children.extend(children.into_iter().map(|c| c.into()));
        self
    }

    pub fn with_text<T: Into<String>>(mut self, text: T) -> Self {
        self.children.push(RenderElement::Text(Text::new(text)));
        self
    }

    pub fn with_item(mut self, item: impl Into<Item>) -> Self {
        self.children.push(RenderElement::Item(item.into()));
        self
    }

    pub fn with_section(mut self, section: impl Into<Section>) -> Self {
        self.children.push(RenderElement::Section(section.into()));
        self
    }

    pub fn into_element(self) -> RenderElement {
        RenderElement::Section(self)
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub tag: String,
    pub attrs: IndexMap<String, AttrValue>,
    pub content: Option<String>,
    pub children: Vec<RenderElement>,
}

impl Item {
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            attrs: IndexMap::new(),
            content: None,
            children: Vec::new(),
        }
    }

    pub fn with_attr<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<AttrValue>,
    {
        self.attrs.insert(key.into(), value.into());
        self
    }

    pub fn with_content<T: Into<String>>(mut self, content: T) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn add_child(mut self, child: impl Into<RenderElement>) -> Self {
        self.children.push(child.into());
        self
    }

    pub fn push_child(&mut self, child: impl Into<RenderElement>) {
        self.children.push(child.into());
    }

    pub fn add_children(
        mut self,
        children: impl IntoIterator<Item = impl Into<RenderElement>>,
    ) -> Self {
        self.children.extend(children.into_iter().map(|c| c.into()));
        self
    }

    pub fn into_element(self) -> RenderElement {
        RenderElement::Item(self)
    }
}

#[derive(Debug, Clone)]
pub struct Text {
    pub content: String,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

impl KeyValue {
    pub fn new<K, V>(key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

pub fn section<T: Into<String>>(tag: T) -> Section {
    Section::new(tag)
}

pub fn item<T: Into<String>>(tag: T) -> Item {
    Item::new(tag)
}

pub fn text<T: Into<String>>(content: T) -> RenderElement {
    RenderElement::Text(Text::new(content))
}

pub fn kv<K, V>(key: K, value: V) -> RenderElement
where
    K: Into<String>,
    V: Into<String>,
{
    RenderElement::KeyValue(KeyValue::new(key, value))
}

pub fn empty() -> RenderElement {
    RenderElement::Empty
}

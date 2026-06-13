use std::borrow::Cow;

use async_graphql::{OutputType, SimpleObject, TypeName};

#[derive(SimpleObject)]
#[graphql(name_type)]
pub struct Connection<T: OutputType> {
    pub edges: Vec<Edge<T>>,
    pub page_info: PageInfo,
}

impl<T: OutputType> TypeName for Connection<T> {
    fn type_name() -> Cow<'static, str> {
        Cow::Owned(format!("{}Connection", T::type_name()))
    }
}

#[derive(SimpleObject)]
#[graphql(name_type)]
pub struct Edge<T: OutputType> {
    pub node: T,
    pub cursor: String,
}

impl<T: OutputType> TypeName for Edge<T> {
    fn type_name() -> Cow<'static, str> {
        Cow::Owned(format!("{}Edge", T::type_name()))
    }
}

#[derive(SimpleObject, Debug, Clone, PartialEq, Eq, Default)]
pub struct PageInfo {
    pub has_next_page: bool,
    pub has_previous_page: bool,
    pub start_cursor: Option<String>,
    pub end_cursor: Option<String>,
}

impl<T: OutputType> Edge<T> {
    pub fn new(node: T, cursor: impl Into<String>) -> Self {
        Self {
            node,
            cursor: cursor.into(),
        }
    }
}

impl<T: OutputType> Connection<T> {
    pub fn forward(edges: Vec<Edge<T>>, has_next_page: bool) -> Self {
        let start_cursor = edges.first().map(|e| e.cursor.clone());
        let end_cursor = edges.last().map(|e| e.cursor.clone());
        Self {
            edges,
            page_info: PageInfo {
                has_next_page,
                has_previous_page: false,
                start_cursor,
                end_cursor,
            },
        }
    }

    pub fn new(edges: Vec<Edge<T>>, page_info: PageInfo) -> Self {
        Self { edges, page_info }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_derives_boundary_cursors() {
        let conn = Connection::forward(vec![Edge::new(1_i32, "c1"), Edge::new(2_i32, "c2")], true);
        assert!(conn.page_info.has_next_page);
        assert!(!conn.page_info.has_previous_page);
        assert_eq!(conn.page_info.start_cursor.as_deref(), Some("c1"));
        assert_eq!(conn.page_info.end_cursor.as_deref(), Some("c2"));
    }

    #[test]
    fn empty_forward_page_has_no_cursors() {
        let conn: Connection<i32> = Connection::forward(vec![], false);
        assert!(!conn.page_info.has_next_page);
        assert_eq!(conn.page_info.start_cursor, None);
        assert_eq!(conn.page_info.end_cursor, None);
    }
}

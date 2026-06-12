//! Cursor-based pagination: a reusable, generic [`Connection`] of `T`.
//!
//! Generalizes the per-service connection (e.g. svc-notifier's
//! `NotificationConnection { nodes, has_next_page }`) into one `Connection<T>`
//! every service reuses, in the Relay-flavoured shape (`edges { node cursor }` +
//! `pageInfo`) so the frontend codegen has a uniform paging contract.
//!
//! ## Per-node GraphQL names (the collision fix)
//!
//! `Connection<T>` and `Edge<T>` are `SimpleObject`s whose GraphQL **name is
//! woven from the node type** (`{Node}Connection` / `{Node}Edge`), via a
//! `TypeName` impl and `#[graphql(name_type)]`. This is the established
//! async-graphql idiom (its own `connection::Connection` names itself from the
//! node) and it is *load-bearing*: a fixed `Connection` name would make two
//! different node types **collide** in one schema. The `tests/schema_mounting.rs`
//! integration test pins that `DocConnection` / `DocEdge` actually appear in the
//! built SDL.
//!
//! The cursor is an **opaque String** the server mints and the client echoes
//! back via `after`; this crate prescribes no encoding (a service may base64 an
//! id + sort key) — only that it is opaque to the client.

use std::borrow::Cow;

use async_graphql::{OutputType, SimpleObject, TypeName};

/// One page of `T`, with its edges and paging metadata. Registers in the schema
/// as `{Node}Connection` (e.g. `DocConnection`).
#[derive(SimpleObject)]
#[graphql(name_type)]
pub struct Connection<T: OutputType> {
    /// The edges in this page (node + cursor each).
    pub edges: Vec<Edge<T>>,
    /// The paging metadata for this page.
    pub page_info: PageInfo,
}

impl<T: OutputType> TypeName for Connection<T> {
    fn type_name() -> Cow<'static, str> {
        Cow::Owned(format!("{}Connection", T::type_name()))
    }
}

/// One element of a [`Connection`]: the node plus its opaque cursor. Registers
/// as `{Node}Edge` (e.g. `DocEdge`).
#[derive(SimpleObject)]
#[graphql(name_type)]
pub struct Edge<T: OutputType> {
    /// The node at this edge.
    pub node: T,
    /// The opaque cursor identifying this edge's position.
    pub cursor: String,
}

impl<T: OutputType> TypeName for Edge<T> {
    fn type_name() -> Cow<'static, str> {
        Cow::Owned(format!("{}Edge", T::type_name()))
    }
}

/// Paging metadata for a [`Connection`]. Forward-only fields are always
/// populated; the backward-paging fields are present for Relay compatibility and
/// default to safe values when a service only pages forward.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq, Default)]
pub struct PageInfo {
    /// Whether a further forward page exists.
    pub has_next_page: bool,
    /// Whether a previous page exists (backward paging).
    pub has_previous_page: bool,
    /// The cursor of the first edge in this page, if any.
    pub start_cursor: Option<String>,
    /// The cursor of the last edge in this page, if any.
    pub end_cursor: Option<String>,
}

impl<T: OutputType> Edge<T> {
    /// Pair a node with its opaque cursor.
    pub fn new(node: T, cursor: impl Into<String>) -> Self {
        Self {
            node,
            cursor: cursor.into(),
        }
    }
}

impl<T: OutputType> Connection<T> {
    /// Build a forward page from already-cursored edges and the
    /// `has_next_page` flag the query computed (typically by fetching `first + 1`
    /// and trimming). `start_cursor` / `end_cursor` are derived from the edges.
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

    /// Build a page from edges and an explicit [`PageInfo`] (when a service
    /// pages both directions).
    pub fn new(edges: Vec<Edge<T>>, page_info: PageInfo) -> Self {
        Self { edges, page_info }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given a forward page of cursored edges, Then start/end cursors are the
    // first and last edge cursors and backward paging is off.
    #[test]
    fn forward_derives_boundary_cursors() {
        let conn = Connection::forward(vec![Edge::new(1_i32, "c1"), Edge::new(2_i32, "c2")], true);
        assert!(conn.page_info.has_next_page);
        assert!(!conn.page_info.has_previous_page);
        assert_eq!(conn.page_info.start_cursor.as_deref(), Some("c1"));
        assert_eq!(conn.page_info.end_cursor.as_deref(), Some("c2"));
    }

    // Given an empty forward page, Then there are no boundary cursors.
    #[test]
    fn empty_forward_page_has_no_cursors() {
        let conn: Connection<i32> = Connection::forward(vec![], false);
        assert!(!conn.page_info.has_next_page);
        assert_eq!(conn.page_info.start_cursor, None);
        assert_eq!(conn.page_info.end_cursor, None);
    }
}

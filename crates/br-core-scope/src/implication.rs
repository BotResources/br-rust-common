use std::collections::HashMap;

use crate::error::ScopeDeclarationError;
use crate::spec::{ScopeSpecV2, ServiceManifest};

pub(crate) fn check_implications(
    manifest: &ServiceManifest,
    scopes: &[ScopeSpecV2],
) -> Result<(), ScopeDeclarationError> {
    let index: HashMap<&str, usize> = scopes
        .iter()
        .enumerate()
        .map(|(position, spec)| (spec.key.as_str(), position))
        .collect();

    for spec in scopes {
        for implied in &spec.implies {
            if !implied.is_owned_by(&manifest.key) {
                return Err(ScopeDeclarationError::ImpliedScopePrefixMismatch {
                    key: spec.key.as_str().to_string(),
                    implied: implied.as_str().to_string(),
                    declaring_service: manifest.key.as_str().to_string(),
                });
            }
            if !index.contains_key(implied.as_str()) {
                return Err(ScopeDeclarationError::UnknownImpliedScope {
                    key: spec.key.as_str().to_string(),
                    implied: implied.as_str().to_string(),
                });
            }
        }
    }

    match find_cycle(scopes, &index) {
        Some(cycle) => Err(ScopeDeclarationError::ImplicationCycle { cycle }),
        None => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Unvisited,
    OnPath,
    Done,
}

fn find_cycle(scopes: &[ScopeSpecV2], index: &HashMap<&str, usize>) -> Option<Vec<String>> {
    let mut marks = vec![Mark::Unvisited; scopes.len()];

    for start in 0..scopes.len() {
        if marks[start] != Mark::Unvisited {
            continue;
        }
        marks[start] = Mark::OnPath;
        let mut path: Vec<(usize, usize)> = vec![(start, 0)];

        while let Some(top) = path.last_mut() {
            let (node, edge) = *top;
            let Some(implied) = scopes[node].implies.get(edge) else {
                marks[node] = Mark::Done;
                path.pop();
                continue;
            };
            top.1 += 1;
            let Some(&next) = index.get(implied.as_str()) else {
                continue;
            };
            match marks[next] {
                Mark::Unvisited => {
                    marks[next] = Mark::OnPath;
                    path.push((next, 0));
                }
                Mark::OnPath => {
                    let entry = path
                        .iter()
                        .position(|&(on_path, _)| on_path == next)
                        .unwrap_or(0);
                    let mut cycle: Vec<String> = path[entry..]
                        .iter()
                        .map(|&(on_path, _)| scopes[on_path].key.as_str().to_string())
                        .collect();
                    cycle.push(scopes[next].key.as_str().to_string());
                    return Some(cycle);
                }
                Mark::Done => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::ScopeKey;
    use crate::service::ServiceKey;

    fn manifest() -> ServiceManifest {
        ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "l", "d")
    }

    fn spec(key: &str, implies: &[&str]) -> ScopeSpecV2 {
        ScopeSpecV2::new(ScopeKey::new(key).unwrap(), "l", "d", false)
            .with_implies(implies.iter().map(|k| ScopeKey::new(*k).unwrap()))
    }

    fn cycle(keys: &[&str]) -> ScopeDeclarationError {
        ScopeDeclarationError::ImplicationCycle {
            cycle: keys.iter().map(|k| k.to_string()).collect(),
        }
    }

    #[test]
    fn accepts_no_implication() {
        let scopes = [spec("notifier:read", &[]), spec("notifier:write", &[])];
        assert_eq!(check_implications(&manifest(), &scopes), Ok(()));
    }

    #[test]
    fn accepts_a_chain_and_a_diamond() {
        let scopes = [
            spec("notifier:admin", &["notifier:write", "notifier:send"]),
            spec("notifier:write", &["notifier:read"]),
            spec("notifier:send", &["notifier:read"]),
            spec("notifier:read", &[]),
        ];
        assert_eq!(check_implications(&manifest(), &scopes), Ok(()));
    }

    #[test]
    fn accepts_an_implied_scope_declared_after_its_implier() {
        let scopes = [
            spec("notifier:admin", &["notifier:read"]),
            spec("notifier:read", &[]),
        ];
        assert_eq!(check_implications(&manifest(), &scopes), Ok(()));
    }

    #[test]
    fn rejects_an_implied_scope_missing_from_the_declaration() {
        let scopes = [spec("notifier:admin", &["notifier:ghost"])];
        assert_eq!(
            check_implications(&manifest(), &scopes),
            Err(ScopeDeclarationError::UnknownImpliedScope {
                key: "notifier:admin".to_string(),
                implied: "notifier:ghost".to_string(),
            })
        );
    }

    #[test]
    fn rejects_an_implied_scope_of_another_service() {
        let scopes = [spec("notifier:admin", &["billing:read"])];
        assert_eq!(
            check_implications(&manifest(), &scopes),
            Err(ScopeDeclarationError::ImpliedScopePrefixMismatch {
                key: "notifier:admin".to_string(),
                implied: "billing:read".to_string(),
                declaring_service: "notifier".to_string(),
            })
        );
    }

    #[test]
    fn reports_the_first_offending_implied_scope_in_list_order() {
        let scopes = [spec("notifier:admin", &["notifier:ghost", "billing:read"])];
        assert!(matches!(
            check_implications(&manifest(), &scopes),
            Err(ScopeDeclarationError::UnknownImpliedScope { .. })
        ));
        let scopes = [spec("notifier:admin", &["billing:read", "notifier:ghost"])];
        assert!(matches!(
            check_implications(&manifest(), &scopes),
            Err(ScopeDeclarationError::ImpliedScopePrefixMismatch { .. })
        ));
    }

    #[test]
    fn rejects_a_self_implication() {
        let scopes = [
            spec("notifier:read", &[]),
            spec("notifier:admin", &["notifier:admin"]),
        ];
        assert_eq!(
            check_implications(&manifest(), &scopes),
            Err(cycle(&["notifier:admin", "notifier:admin"]))
        );
    }

    #[test]
    fn rejects_a_two_scope_cycle() {
        let scopes = [
            spec("notifier:a", &["notifier:b"]),
            spec("notifier:b", &["notifier:a"]),
        ];
        assert_eq!(
            check_implications(&manifest(), &scopes),
            Err(cycle(&["notifier:a", "notifier:b", "notifier:a"]))
        );
    }

    #[test]
    fn rejects_a_longer_cycle_and_reports_only_the_cycle() {
        let scopes = [
            spec("notifier:root", &["notifier:a"]),
            spec("notifier:a", &["notifier:b"]),
            spec("notifier:b", &["notifier:c", "notifier:leaf"]),
            spec("notifier:c", &["notifier:a"]),
            spec("notifier:leaf", &[]),
        ];
        assert_eq!(
            check_implications(&manifest(), &scopes),
            Err(cycle(&[
                "notifier:a",
                "notifier:b",
                "notifier:c",
                "notifier:a"
            ]))
        );
    }

    #[test]
    fn rejects_a_cycle_reached_only_from_a_later_scope() {
        let scopes = [
            spec("notifier:read", &[]),
            spec("notifier:write", &["notifier:read"]),
            spec("notifier:x", &["notifier:y"]),
            spec("notifier:y", &["notifier:z"]),
            spec("notifier:z", &["notifier:x"]),
        ];
        assert_eq!(
            check_implications(&manifest(), &scopes),
            Err(cycle(&[
                "notifier:x",
                "notifier:y",
                "notifier:z",
                "notifier:x"
            ]))
        );
    }

    #[test]
    fn a_shared_descendant_is_not_a_cycle() {
        let scopes = [
            spec("notifier:a", &["notifier:c"]),
            spec("notifier:b", &["notifier:c"]),
            spec("notifier:c", &[]),
        ];
        assert_eq!(check_implications(&manifest(), &scopes), Ok(()));
    }
}

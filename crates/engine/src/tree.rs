//! Lookups and edits on the typed tree of areas and tasks.

use crate::stamps::alive;
use balancecraft_model::{Area, TaskNode};

/// Searches `node` and its subtree, tombstones included.
pub fn search<'a>(node: &'a TaskNode, id: &str) -> Option<&'a TaskNode> {
    if node.id == id {
        return Some(node);
    }
    node.children.iter().find_map(|c| search(c, id))
}

/// A task anywhere in the tree, tombstones included, with its area.
pub fn find_task<'a>(areas: &'a [Area], id: &str) -> Option<(&'a TaskNode, &'a Area)> {
    areas.iter().find_map(|a| a.children.iter().find_map(|c| search(c, id)).map(|n| (n, a)))
}

fn search_visible<'a>(node: &'a TaskNode, id: &str) -> Option<&'a TaskNode> {
    if !alive(node.meta.as_ref()) {
        return None;
    }
    if node.id == id {
        return Some(node);
    }
    node.children.iter().find_map(|c| search_visible(c, id))
}

/// A visible task (it and all its ancestors are live) with its area.
pub fn find_visible_task<'a>(areas: &'a [Area], id: &str) -> Option<(&'a TaskNode, &'a Area)> {
    areas
        .iter()
        .filter(|a| alive(a.meta.as_ref()))
        .find_map(|a| a.children.iter().find_map(|c| search_visible(c, id)).map(|n| (n, a)))
}

/// A visible area.
pub fn find_visible_area<'a>(areas: &'a [Area], id: &str) -> Option<&'a Area> {
    areas.iter().find(|a| a.id == id && alive(a.meta.as_ref()))
}

/// True when `id` is a visible area or a visible task: a place that can take
/// a new child.
pub fn is_visible_parent(areas: &[Area], id: &str) -> bool {
    find_visible_area(areas, id).is_some() || find_visible_task(areas, id).is_some()
}

/// Most levels of tasks under an area. A document stays well inside what the
/// file reader accepts (128 levels of JSON, with the task's `_meta` and the
/// replica's envelope around it), so a file BalanceCraft writes always opens.
pub const MAX_DEPTH: usize = 32;

/// Level of the area or task `id`: 0 for an area, 1 for its tasks, and so on.
pub fn level(areas: &[Area], id: &str) -> Option<usize> {
    fn walk(nodes: &[TaskNode], id: &str, at: usize) -> Option<usize> {
        nodes.iter().find_map(|n| if n.id == id { Some(at) } else { walk(&n.children, id, at.saturating_add(1)) })
    }
    if areas.iter().any(|a| a.id == id) {
        return Some(0);
    }
    areas.iter().find_map(|a| walk(&a.children, id, 1))
}

/// Levels of `node` and its subtree: 1 for a task without children.
pub fn height(node: &TaskNode) -> usize {
    node.children.iter().map(height).max().unwrap_or(0).saturating_add(1)
}

/// True when `node` (with its subtree) can go under the area or task `parent_id`
/// without passing [`MAX_DEPTH`].
pub fn fits_under(areas: &[Area], parent_id: &str, node_height: usize) -> bool {
    level(areas, parent_id).is_some_and(|l| l.saturating_add(node_height) <= MAX_DEPTH)
}

/// Applies `f` to every task with id `id`, at any depth. Returns whether one
/// was found.
pub fn update_task(nodes: &mut [TaskNode], id: &str, f: &mut dyn FnMut(&mut TaskNode)) -> bool {
    let mut found = false;
    for n in nodes {
        if n.id == id {
            f(n);
            found = true;
        } else if update_task(&mut n.children, id, f) {
            found = true;
        }
    }
    found
}

/// The children list of the area or task `id`, tombstones included.
pub fn children_of_mut<'a>(areas: &'a mut [Area], id: &str) -> Option<&'a mut Vec<TaskNode>> {
    fn in_nodes<'a>(nodes: &'a mut [TaskNode], id: &str) -> Option<&'a mut Vec<TaskNode>> {
        for n in nodes {
            if n.id == id {
                return Some(&mut n.children);
            }
            if let Some(found) = in_nodes(&mut n.children, id) {
                return Some(found);
            }
        }
        None
    }
    if let Some(pos) = areas.iter().position(|a| a.id == id) {
        return areas.get_mut(pos).map(|a| &mut a.children);
    }
    areas.iter_mut().find_map(|a| in_nodes(&mut a.children, id))
}

/// The children of the area or task `id`, tombstones included.
pub fn children_of<'a>(areas: &'a [Area], id: &str) -> Option<&'a [TaskNode]> {
    if let Some(a) = areas.iter().find(|a| a.id == id) {
        return Some(&a.children);
    }
    find_task(areas, id).map(|(n, _)| n.children.as_slice())
}

/// Removes the first task with id `id` from the tree and returns it.
pub fn extract(areas: &mut [Area], id: &str) -> Option<TaskNode> {
    fn in_nodes(nodes: &mut Vec<TaskNode>, id: &str) -> Option<TaskNode> {
        if let Some(pos) = nodes.iter().position(|n| n.id == id) {
            return Some(nodes.remove(pos));
        }
        nodes.iter_mut().find_map(|n| in_nodes(&mut n.children, id))
    }
    areas.iter_mut().find_map(|a| in_nodes(&mut a.children, id))
}

/// Path of names from the area to the task, for labels: "Area › Branch › Task".
pub fn path_names(areas: &[Area], id: &str) -> Option<Vec<String>> {
    fn walk(node: &TaskNode, id: &str, path: &mut Vec<String>) -> bool {
        path.push(node.name.clone());
        if node.id == id || node.children.iter().any(|c| walk(c, id, path)) {
            return true;
        }
        path.pop();
        false
    }
    for a in areas {
        let mut path = vec![a.name.clone()];
        if a.children.iter().any(|c| walk(c, id, &mut path)) {
            return Some(path);
        }
    }
    None
}

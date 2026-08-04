//! Category management.
//!
//! `CategoryManager` sits between the CLI and the `Storage` trait and owns all
//! category behaviour described in the README: creating/renaming/deleting
//! categories, the magic "Uncategorized" category (ID 0), custom ordering, and
//! the category context set by `trt category use` which persists between
//! runs via `StorageData::current_category`.

use crate::models::{Category, CategoryError, StorageError};
use crate::storage::Storage;
use chrono::Utc;

/// The magic category that owns every task with no real category.
pub const UNCATEGORIZED_ID: u64 = 0;
const UNCATEGORIZED_NAME: &str = "Uncategorized";

/// Builds the synthesized "Uncategorized" category. It is never a real row
/// in storage (`get_next_category_id` never hands out ID 0), so every place
/// that needs to represent it - `list_categories` below, and
/// `get_category`/`get_category_by_name` for task commands resolving
/// `--category Uncategorized` / `--category 0` - builds it from this one
/// place rather than duplicating the literal.
fn uncategorized_category() -> Category {
    Category {
        id: UNCATEGORIZED_ID,
        name: UNCATEGORIZED_NAME.to_string(),
        description: None,
        order: 0,
        created_at: Utc::now(),
    }
}

/// Whether `name` collides with the synthesized "Uncategorized" category.
/// Case-insensitive, matching the duplicate-name rule applied to real
/// categories.
fn is_reserved_category_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(UNCATEGORIZED_NAME)
}

pub struct CategoryManager<'a> {
    storage: &'a dyn Storage,
    current_category: Option<u64>,
}

impl<'a> CategoryManager<'a> {
    pub fn new(storage: &'a dyn Storage) -> Self {
        let current_category = storage.load().ok().and_then(|data| data.current_category);
        Self {
            storage,
            current_category,
        }
    }

    pub fn add_category(
        &mut self,
        name: String,
        description: Option<String>,
    ) -> Result<u64, CategoryError> {
        let mut category = Category::new(name.clone(), description)?;
        let mut data = self.storage.load()?;

        // "Uncategorized" is synthesized rather than stored, so it is absent
        // from the duplicate scan below and would otherwise be creatable as a
        // real, separate category. That is worse than a cosmetic duplicate in
        // `category list`: name lookups resolve the synthesized ID 0 first
        // (see `get_category_by_name`), so the real one could never be
        // targeted by name again.
        if is_reserved_category_name(&name) {
            return Err(CategoryError::DuplicateName(name));
        }

        // Check for duplicate names
        if data
            .categories
            .iter()
            .any(|c| c.name.to_lowercase() == name.to_lowercase())
        {
            return Err(CategoryError::DuplicateName(name));
        }

        // Get next available ID
        category.id = self.storage.get_next_category_id()?;

        // Set default order to match ID
        category.set_order(category.id as u32);

        data.categories.push(category.clone());
        self.storage.save(&data)?;

        Ok(category.id)
    }

    pub fn delete_category(
        &mut self,
        category_id: u64,
        new_category_id: Option<u64>,
    ) -> Result<(), StorageError> {
        let mut data = self.storage.load()?;

        // Check if category exists
        if !data.categories.iter().any(|c| c.id == category_id) {
            return Err(StorageError::Storage(format!(
                "Category with id {} not found",
                category_id
            )));
        }

        // If new_category_id is provided, move all tasks to that category,
        // otherwise send them to Uncategorized.
        let destination = match new_category_id {
            Some(new_id) => {
                if !data.categories.iter().any(|c| c.id == new_id) {
                    return Err(StorageError::Storage(format!(
                        "New category with id {} not found",
                        new_id
                    )));
                }
                new_id
            }
            None => UNCATEGORIZED_ID,
        };

        for task in data.tasks.iter_mut() {
            if task.category_id == category_id {
                task.category_id = destination;
            }
        }

        // Remove the category. Its ID becomes available again - see
        // `Storage::get_next_category_id`.
        data.categories.retain(|c| c.id != category_id);

        // Clear current category context if it was deleted
        if self.current_category == Some(category_id) {
            self.current_category = None;
            data.current_category = None;
        }

        self.storage.save(&data)
    }

    pub fn update_category(
        &mut self,
        category_id: u64,
        new_name: String,
    ) -> Result<(), StorageError> {
        let mut data = self.storage.load()?;

        // Renaming onto the synthesized category's name has the same problem
        // as creating it outright - see `add_category`.
        if is_reserved_category_name(&new_name) {
            return Err(StorageError::DuplicateCategory(new_name));
        }

        // Check for duplicate names
        if data
            .categories
            .iter()
            .any(|c| c.name.to_lowercase() == new_name.to_lowercase())
        {
            return Err(StorageError::DuplicateCategory(new_name));
        }

        if let Some(category) = data.categories.iter_mut().find(|c| c.id == category_id) {
            category
                .update_name(new_name)
                .map_err(|e| StorageError::Model(e.to_string()))?;
            self.storage.save(&data)?;
            Ok(())
        } else {
            Err(StorageError::Storage(format!(
                "Category with id {} not found",
                category_id
            )))
        }
    }

    /// Lists every category, always including the magic "Uncategorized"
    /// category first, sorted by custom order and then by name.
    pub fn list_categories(&self) -> Result<Vec<Category>, StorageError> {
        let mut categories = self.storage.get_all_categories()?;

        // Drop any stored category that collides with the magic ID, then
        // synthesize "Uncategorized" so it is always present and always first.
        categories.retain(|c| c.id != UNCATEGORIZED_ID);
        categories.push(uncategorized_category());

        // Sort by order first, then by name
        categories.sort_by(|a, b| a.order.cmp(&b.order).then(a.name.cmp(&b.name)));

        Ok(categories)
    }

    pub fn use_category(&mut self, category_id: u64) -> Result<(), StorageError> {
        let mut data = self.storage.load()?;
        if data.categories.iter().any(|c| c.id == category_id) {
            self.current_category = Some(category_id);
            data.current_category = Some(category_id);
            self.storage.save(&data)?;
            Ok(())
        } else {
            Err(StorageError::Storage(format!(
                "Category with id {} not found",
                category_id
            )))
        }
    }

    pub fn clear_category_context(&mut self) -> Result<(), StorageError> {
        self.current_category = None;
        let mut data = self.storage.load()?;
        data.current_category = None;
        self.storage.save(&data)
    }

    /// The category commands operate against, falling back to Uncategorized
    /// when no context has been set.
    pub fn get_current_category(&self) -> Option<u64> {
        Some(self.current_category.unwrap_or(UNCATEGORIZED_ID))
    }

    /// Whether a category context has actually been set via `category use`,
    /// as distinct from `get_current_category`'s "nothing set -> defaults to
    /// Uncategorized" fallback. Task commands that need an *explicit*
    /// context (`check all`/`uncheck all`, the simple `move` syntax) rely on
    /// this distinction: silently falling back to Uncategorized here would
    /// mean operating on the wrong tasks whenever the user simply never
    /// picked a context, rather than telling them to set one.
    pub fn has_explicit_category_context(&self) -> bool {
        self.current_category.is_some()
    }

    /// Looks up a category by name, including the synthesized "Uncategorized"
    /// category - it is never a real row in storage, so without this
    /// special case task commands would have no way to target it via
    /// `--category Uncategorized`.
    pub fn get_category_by_name(&self, name: &str) -> Result<Option<Category>, StorageError> {
        if name.eq_ignore_ascii_case(UNCATEGORIZED_NAME) {
            return Ok(Some(uncategorized_category()));
        }
        self.storage.get_category_by_name(name)
    }

    /// Looks up a category by ID, including the synthesized "Uncategorized"
    /// category (ID 0) - see `get_category_by_name`.
    pub fn get_category(&self, id: u64) -> Result<Option<Category>, StorageError> {
        if id == UNCATEGORIZED_ID {
            return Ok(Some(uncategorized_category()));
        }
        self.storage.get_category(id)
    }

    /// Sets a single category's position in `category list`.
    ///
    /// `new_order` is stored verbatim in `Category.order`, which is exactly
    /// the sort key `list_categories` uses (order, then name) - so this is a
    /// direct assignment, not an "insert and push everyone else down"
    /// operation. Positions are 1-based (see the CLI's `category order`
    /// doc comment for why), and `0` is reserved for the synthesized
    /// "Uncategorized" category's fixed order - see `uncategorized_category`.
    /// This function does not enforce that reservation itself, so a caller
    /// could still hand a *real* category `order == 0` and tie it with
    /// Uncategorized for the top slot; `main::run_category_command` is what
    /// actually rejects position `0` before it reaches here, because the
    /// CLI is the only caller that needs to speak to a user, and other
    /// callers (tests, a future scripting surface) should stay free to set
    /// whatever order they like.
    ///
    /// Two categories sharing an order value is not an error: they tie, and
    /// `list_categories`'s secondary sort key (name) decides which comes
    /// first. Nothing is displaced to make room.
    ///
    /// Targeting `UNCATEGORIZED_ID` itself fails - not because this checks
    /// for it, but because no stored row with that ID exists to update
    /// (`list_categories` synthesizes it fresh on every call). The CLI
    /// layer intercepts that case earlier to give a clearer error than the
    /// generic "not found" this falls through to.
    pub fn set_category_order(
        &mut self,
        category_id: u64,
        new_order: u32,
    ) -> Result<(), StorageError> {
        let mut data = self.storage.load()?;
        if let Some(category) = data.categories.iter_mut().find(|c| c.id == category_id) {
            category.set_order(new_order);
            self.storage.save(&data)
        } else {
            Err(StorageError::Storage(format!(
                "Category with id {} not found",
                category_id
            )))
        }
    }

    /// Sets the order of several categories at once by listing them in the
    /// order they should appear.
    ///
    /// Assigns positions `1..=category_ids.len()`, in list order. This is
    /// 1-based for the same reason `set_category_order` is: `0` is the
    /// synthesized "Uncategorized" category's fixed, unassignable order.
    /// A 0-based assignment would have handed the *first* listed category
    /// that same value and let it race Uncategorized alphabetically for the
    /// top slot on a tie, quietly breaking `list_categories`'s "always
    /// first" guarantee for whichever category happened to be listed first.
    ///
    /// Categories not named here are left untouched - their existing
    /// `order` stays whatever it was. That is a deliberate partial update,
    /// not an omission: a category with a low pre-existing order can
    /// therefore still sort between, or even before, the ones just
    /// renumbered, rather than always being pushed after them. Renumbering
    /// the rest to guarantee "unlisted always sorts last" would mean this
    /// function reaching into and rewriting categories the caller never
    /// mentioned, which is a bigger surprise than a partial reorder leaving
    /// a partial order. A caller that wants full control over the result
    /// lists every category.
    pub fn reorder_categories(&mut self, category_ids: Vec<u64>) -> Result<(), StorageError> {
        let mut data = self.storage.load()?;

        // Validate all categories exist
        for id in &category_ids {
            if !data.categories.iter().any(|c| c.id == *id) {
                return Err(StorageError::Storage(format!(
                    "Category with id {} not found",
                    id
                )));
            }
        }

        // Update orders, 1-based - see the doc comment above for why 0 is
        // never handed out here.
        for (index, id) in category_ids.iter().enumerate() {
            if let Some(category) = data.categories.iter_mut().find(|c| c.id == *id) {
                category.set_order(index as u32 + 1);
            }
        }

        self.storage.save(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_utils::TestStorage;

    #[test]
    fn test_add_category() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        let result = manager.add_category("Test".to_string(), None);
        assert!(result.is_ok());

        let categories = manager
            .list_categories()
            .expect("Failed to list categories");
        assert_eq!(categories.len(), 2); // Uncategorized + Test
        assert!(categories.iter().any(|c| c.name == "Test"));
    }

    #[test]
    fn test_delete_category() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        // Add a category first
        let id = manager
            .add_category("Test".to_string(), None)
            .expect("Failed to add category");

        // Delete it
        let result = manager.delete_category(id, None);
        assert!(result.is_ok());

        let categories = manager
            .list_categories()
            .expect("Failed to list categories");
        assert_eq!(categories.len(), 1); // Only Uncategorized remains
        assert!(categories.iter().any(|c| c.name == "Uncategorized"));
    }

    #[test]
    fn test_update_category() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        // Add a category first
        let id = manager
            .add_category("Test".to_string(), None)
            .expect("Failed to add category");

        // Update it
        let result = manager.update_category(id, "Updated".to_string());
        assert!(result.is_ok());

        let categories = manager
            .list_categories()
            .expect("Failed to list categories");
        assert_eq!(categories.len(), 2); // Uncategorized + Updated
        assert!(categories.iter().any(|c| c.name == "Updated"));
    }

    #[test]
    fn test_category_context() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        // Initially should be Uncategorized (0)
        assert_eq!(manager.get_current_category(), Some(0));

        // Add a category
        let id = manager
            .add_category("Test".to_string(), None)
            .expect("Failed to add category");

        // Set as current
        let result = manager.use_category(id);
        assert!(result.is_ok());
        assert_eq!(manager.get_current_category(), Some(id));

        // The context must survive a restart: a fresh manager over the same
        // storage picks it back up.
        let reloaded = CategoryManager::new(test_storage.storage());
        assert_eq!(reloaded.get_current_category(), Some(id));

        // Clear context
        let result = manager.clear_category_context();
        assert!(result.is_ok());
        assert_eq!(manager.get_current_category(), Some(0)); // Back to Uncategorized
    }

    #[test]
    fn test_category_ordering() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        // Add categories
        let id1 = manager
            .add_category("A".to_string(), None)
            .expect("Failed to add category");
        let id2 = manager
            .add_category("B".to_string(), None)
            .expect("Failed to add category");
        let id3 = manager
            .add_category("C".to_string(), None)
            .expect("Failed to add category");

        // Set custom order
        manager
            .set_category_order(id2, 1)
            .expect("Failed to set order");
        manager
            .set_category_order(id1, 2)
            .expect("Failed to set order");
        manager
            .set_category_order(id3, 3)
            .expect("Failed to set order");

        let categories = manager
            .list_categories()
            .expect("Failed to list categories");
        assert_eq!(categories.len(), 4); // Uncategorized + A + B + C

        // Uncategorized is always first (order 0)
        assert_eq!(categories[0].name, "Uncategorized");
        assert_eq!(categories[0].order, 0);

        // Then our custom order
        assert_eq!(categories[1].name, "B");
        assert_eq!(categories[1].order, 1);
        assert_eq!(categories[2].name, "A");
        assert_eq!(categories[2].order, 2);
        assert_eq!(categories[3].name, "C");
        assert_eq!(categories[3].order, 3);
    }

    #[test]
    fn test_default_category_order() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        // Add categories
        let _id1 = manager
            .add_category("A".to_string(), None)
            .expect("Failed to add category");
        let _id2 = manager
            .add_category("B".to_string(), None)
            .expect("Failed to add category");
        let _id3 = manager
            .add_category("C".to_string(), None)
            .expect("Failed to add category");

        // Verify that categories are ordered by their IDs by default
        let categories = manager
            .list_categories()
            .expect("Failed to list categories");
        assert_eq!(categories.len(), 4); // Uncategorized + A + B + C

        // Check that orders match IDs
        for category in categories {
            assert_eq!(category.order, category.id as u32);
        }
    }

    #[test]
    fn test_duplicate_names() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        // Add a category
        let result = manager.add_category("Test".to_string(), None);
        assert!(result.is_ok());

        // Try to add another with the same name
        let result = manager.add_category("Test".to_string(), None);
        assert!(result.is_err());

        let categories = manager
            .list_categories()
            .expect("Failed to list categories");
        assert_eq!(categories.len(), 2); // Uncategorized + Test
    }

    /// README: "when deleting a category it is removed and its ID is made
    /// available again."
    #[test]
    fn test_deleted_category_id_is_reused() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        let a = manager.add_category("A".to_string(), None).unwrap();
        let b = manager.add_category("B".to_string(), None).unwrap();
        let c = manager.add_category("C".to_string(), None).unwrap();
        assert_eq!((a, b, c), (1, 2, 3));

        manager.delete_category(b, None).unwrap();

        // The freed ID 2 is handed out again rather than jumping to 4.
        let d = manager.add_category("D".to_string(), None).unwrap();
        assert_eq!(d, 2);
    }

    /// `Uncategorized` is never a real row in storage, but task commands
    /// need to be able to resolve `--category Uncategorized` / `--category
    /// 0` the same way they resolve any real category name or ID.
    #[test]
    fn get_category_resolves_the_synthesized_uncategorized_category() {
        let test_storage = TestStorage::new();
        let manager = CategoryManager::new(test_storage.storage());

        let by_id = manager.get_category(UNCATEGORIZED_ID).unwrap().unwrap();
        assert_eq!(by_id.name, "Uncategorized");

        let by_name = manager
            .get_category_by_name("uncategorized")
            .unwrap()
            .unwrap();
        assert_eq!(by_name.id, UNCATEGORIZED_ID);
    }

    /// The synthesized "Uncategorized" category is not a stored row, so the
    /// duplicate-name scan cannot see it. Without an explicit guard a user
    /// could create a *second*, real category with that name - and because
    /// `get_category_by_name` resolves the synthesized ID 0 first, the real
    /// one would be permanently unreachable by name (verified against the
    /// built binary before this guard existed: `category add Uncategorized`
    /// succeeded with ID 1, `category list` showed the name twice, and
    /// `add <task> --category Uncategorized` silently targeted ID 0).
    #[test]
    fn uncategorized_name_is_reserved() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        assert!(manager
            .add_category("Uncategorized".to_string(), None)
            .is_err());
        // Case- and whitespace-insensitively, matching the duplicate rule.
        assert!(manager
            .add_category("  uncategorized ".to_string(), None)
            .is_err());

        // Only the synthesized entry exists.
        let categories = manager.list_categories().unwrap();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].id, UNCATEGORIZED_ID);

        // Renaming onto it is refused for the same reason.
        let id = manager.add_category("Work".to_string(), None).unwrap();
        assert!(manager
            .update_category(id, "Uncategorized".to_string())
            .is_err());
    }

    /// `reorder_categories` assigns 1-based positions in list order, not
    /// 0-based - see the doc comment on `reorder_categories` for why 0
    /// staying unassigned matters.
    #[test]
    fn test_reorder_categories_assigns_one_based_positions() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        let a = manager.add_category("A".to_string(), None).unwrap();
        let b = manager.add_category("B".to_string(), None).unwrap();
        let c = manager.add_category("C".to_string(), None).unwrap();

        manager.reorder_categories(vec![c, a, b]).unwrap();

        let categories = manager.list_categories().unwrap();
        let order_of = |name: &str| {
            categories
                .iter()
                .find(|cat| cat.name == name)
                .unwrap()
                .order
        };
        assert_eq!(order_of("C"), 1);
        assert_eq!(order_of("A"), 2);
        assert_eq!(order_of("B"), 3);

        // Uncategorized's order (0) was never handed out, so it is still
        // sorted first ahead of everything reordered above it.
        assert_eq!(categories[0].name, "Uncategorized");
        assert_eq!(categories[0].order, 0);
    }

    /// A partial `reorder_categories` call renumbers only the categories it
    /// is given; anything left out keeps its prior order rather than being
    /// pushed after the renumbered ones.
    #[test]
    fn test_reorder_categories_partial_list_leaves_others_untouched() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        let a = manager.add_category("A".to_string(), None).unwrap(); // default order 1
        let b = manager.add_category("B".to_string(), None).unwrap(); // default order 2
        let c = manager.add_category("C".to_string(), None).unwrap(); // default order 3

        // Only reorder B and C; A is left alone.
        manager.reorder_categories(vec![c, b]).unwrap();

        let categories = manager.list_categories().unwrap();
        let order_of = |id: u64| categories.iter().find(|cat| cat.id == id).unwrap().order;

        assert_eq!(order_of(a), 1); // untouched
        assert_eq!(order_of(c), 1); // newly assigned - ties with A
        assert_eq!(order_of(b), 2);
    }

    /// Setting two categories to the same order does not error or displace
    /// anything; `list_categories`'s secondary sort key (name) breaks the
    /// tie deterministically.
    #[test]
    fn test_set_category_order_collision_breaks_tie_by_name() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        let a = manager.add_category("Zebra".to_string(), None).unwrap();
        let b = manager.add_category("Apple".to_string(), None).unwrap();

        manager.set_category_order(a, 5).unwrap();
        manager.set_category_order(b, 5).unwrap();

        let categories = manager.list_categories().unwrap();
        let names: Vec<&str> = categories.iter().map(|c| c.name.as_str()).collect();
        // Both tied at order 5, so "Apple" sorts before "Zebra".
        assert_eq!(names, vec!["Uncategorized", "Apple", "Zebra"]);
    }

    /// `Uncategorized` is never a stored row (see `uncategorized_category`),
    /// so there is nothing for either ordering function to update - both
    /// fail rather than silently no-op.
    #[test]
    fn test_ordering_functions_reject_uncategorized() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());
        manager.add_category("Work".to_string(), None).unwrap();

        assert!(manager.set_category_order(UNCATEGORIZED_ID, 1).is_err());
        assert!(manager.reorder_categories(vec![UNCATEGORIZED_ID]).is_err());
    }

    /// `has_explicit_category_context` must not be fooled by
    /// `get_current_category`'s "nothing set" fallback to Uncategorized.
    #[test]
    fn has_explicit_category_context_distinguishes_unset_from_uncategorized_fallback() {
        let test_storage = TestStorage::new();
        let mut manager = CategoryManager::new(test_storage.storage());

        assert!(!manager.has_explicit_category_context());
        assert_eq!(manager.get_current_category(), Some(UNCATEGORIZED_ID));

        let id = manager.add_category("Work".to_string(), None).unwrap();
        manager.use_category(id).unwrap();
        assert!(manager.has_explicit_category_context());
    }
}

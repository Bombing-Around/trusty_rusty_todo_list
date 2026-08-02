//! Category management.
//!
//! `CategoryManager` sits between the CLI and the `Storage` trait and owns all
//! category behaviour described in the README: creating/renaming/deleting
//! categories, the magic "Uncategorized" category (ID 0), custom ordering, and
//! the category context set by `trtodo category use` which persists between
//! runs via `StorageData::current_category`.

use crate::models::{Category, CategoryError, StorageError};
use crate::storage::Storage;
use chrono::Utc;

/// The magic category that owns every task with no real category.
pub const UNCATEGORIZED_ID: u64 = 0;
const UNCATEGORIZED_NAME: &str = "Uncategorized";

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
        categories.push(Category {
            id: UNCATEGORIZED_ID,
            name: UNCATEGORIZED_NAME.to_string(),
            description: None,
            order: 0,
            created_at: Utc::now(),
        });

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

    pub fn get_category_by_name(&self, name: &str) -> Result<Option<Category>, StorageError> {
        self.storage.get_category_by_name(name)
    }

    pub fn get_category(&self, id: u64) -> Result<Option<Category>, StorageError> {
        self.storage.get_category(id)
    }

    // Not yet reachable from the CLI: `trtodo category order` / `reorder` are
    // not part of the command surface in `cli.rs` yet.
    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

        // Update orders
        for (order, id) in category_ids.iter().enumerate() {
            if let Some(category) = data.categories.iter_mut().find(|c| c.id == *id) {
                category.set_order(order as u32);
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

    /// Issue #16 / README: "when deleting a category it is removed and its ID
    /// is made available again."
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
}

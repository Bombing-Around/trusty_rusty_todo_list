use crate::models::{Category, CategoryError, StorageError};
use crate::storage::Storage;

pub struct CategoryManager<'a> {
    storage: &'a dyn Storage,
    current_category: Option<u64>,
}

impl<'a> CategoryManager<'a> {
    pub fn new(storage: &'a dyn Storage) -> Self {
        let current_category = storage.load().ok()
            .and_then(|data| data.current_category);
        Self {
            storage,
            current_category,
        }
    }

    pub fn add_category(&mut self, name: String, description: Option<String>) -> Result<u64, CategoryError> {
        let mut category = Category::new(name.clone(), description)?;
        let mut data = self.storage.load()?;

        // Check for duplicate names
        if data.categories.iter().any(|c| c.name.to_lowercase() == name.to_lowercase()) {
            return Err(CategoryError::DuplicateName(name));
        }

        // Get next available ID
        category.id = self.storage.get_next_category_id()?;
        data.categories.push(category.clone());
        self.storage.save(&data)?;

        Ok(category.id)
    }

    pub fn delete_category(&mut self, category_id: u64, new_category_id: Option<u64>) -> Result<(), StorageError> {
        let mut data = self.storage.load()?;
        
        // Check if category exists
        if !data.categories.iter().any(|c| c.id == category_id) {
            return Err(StorageError::Storage(format!("Category with id {} not found", category_id)));
        }

        // If new_category_id is provided, move all tasks to that category
        if let Some(new_id) = new_category_id {
            if !data.categories.iter().any(|c| c.id == new_id) {
                return Err(StorageError::Storage(format!("New category with id {} not found", new_id)));
            }
            
            for task in data.tasks.iter_mut() {
                if task.category_id == category_id {
                    task.category_id = new_id;
                }
            }
        } else {
            // Move tasks to uncategorized (category_id = 0)
            for task in data.tasks.iter_mut() {
                if task.category_id == category_id {
                    task.category_id = 0;
                }
            }
        }

        // Remove the category
        data.categories.retain(|c| c.id != category_id);

        // Clear current category context if it was deleted
        if self.current_category == Some(category_id) {
            self.current_category = None;
            data.current_category = None;
        }

        self.storage.save(&data)
    }

    pub fn update_category(&mut self, category_id: u64, new_name: String) -> Result<(), StorageError> {
        let mut data = self.storage.load()?;
        
        // Check for duplicate names
        if data.categories.iter().any(|c| c.name.to_lowercase() == new_name.to_lowercase()) {
            return Err(StorageError::DuplicateCategory(new_name));
        }

        if let Some(category) = data.categories.iter_mut().find(|c| c.id == category_id) {
            category.update_name(new_name)?;
            self.storage.save(&data)?;
            Ok(())
        } else {
            Err(StorageError::Storage(format!("Category with id {} not found", category_id)))
        }
    }

    pub fn list_categories(&self) -> Result<Vec<Category>, StorageError> {
        Ok(self.storage.get_all_categories()?)
    }

    pub fn use_category(&mut self, category_id: u64) -> Result<(), StorageError> {
        let mut data = self.storage.load()?;
        if data.categories.iter().any(|c| c.id == category_id) {
            self.current_category = Some(category_id);
            data.current_category = Some(category_id);
            self.storage.save(&data)?;
            Ok(())
        } else {
            Err(StorageError::Storage(format!("Category with id {} not found", category_id)))
        }
    }

    pub fn clear_category_context(&mut self) -> Result<(), StorageError> {
        self.current_category = None;
        let mut data = self.storage.load()?;
        data.current_category = None;
        self.storage.save(&data)
    }

    pub fn get_current_category(&self) -> Option<u64> {
        self.current_category
    }

    pub fn get_category_by_name(&self, name: &str) -> Result<Option<Category>, StorageError> {
        self.storage.get_category_by_name(name)
    }

    pub fn get_category(&self, id: u64) -> Result<Option<Category>, StorageError> {
        self.storage.get_category(id)
    }
}

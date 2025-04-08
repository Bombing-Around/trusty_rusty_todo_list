use assert_cmd::Command;
use tempfile::NamedTempFile;

#[test]
fn test_reset_command_rejected() {
    let mut cmd = Command::cargo_bin("trusty_rusty_todo_list").unwrap();
    let child = cmd
        .args(["config", "reset"])
        .timeout(std::time::Duration::from_secs(2))
        .write_stdin("n\n")
        .assert()
        .success();

    let output = String::from_utf8(child.get_output().stdout.clone()).unwrap();
    assert!(output.contains("Warning: This will delete all tasks and categories"));
    assert!(output.contains("Operation cancelled"));
}

#[test]
fn test_reset_command_accepted() {
    // Create a temporary config file
    let temp_file = NamedTempFile::new().unwrap();
    let config_path = temp_file.path().to_str().unwrap();
    
    // First add a category to have some data
    let mut cmd = Command::cargo_bin("trusty_rusty_todo_list").unwrap();
    cmd.env("TRTODO_CONFIG", config_path)
        .args(["category", "add", "TestCategory"])
        .assert()
        .success();

    // Now reset the database
    let mut cmd = Command::cargo_bin("trusty_rusty_todo_list").unwrap();
    let child = cmd
        .env("TRTODO_CONFIG", config_path)
        .args(["config", "reset"])
        .timeout(std::time::Duration::from_secs(2))
        .write_stdin("y\n")
        .assert()
        .success();

    let output = String::from_utf8(child.get_output().stdout.clone()).unwrap();
    assert!(output.contains("Warning: This will delete all tasks and categories"));
    assert!(output.contains("Database has been reset to initial state with default categories"));

    // Verify that only default categories exist now
    let mut cmd = Command::cargo_bin("trusty_rusty_todo_list").unwrap();
    let list_output = cmd
        .env("TRTODO_CONFIG", config_path)
        .args(["category", "list"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let list_output = String::from_utf8(list_output).unwrap();
    assert!(list_output.contains("Home"));
    assert!(list_output.contains("Work"));
    assert!(!list_output.contains("TestCategory"));
}

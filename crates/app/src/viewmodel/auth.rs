//! Authentication view model

use std::sync::Arc;

use slint::ComponentHandle;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use exom_core::{User, Session};

use crate::state::AppState;
use crate::MainWindow;

pub fn setup_auth_bindings(window: &MainWindow, state: Arc<AppState>) {
    // Login callback
    let state_login = state.clone();
    let window_weak = window.as_weak();
    window.on_login(move |username, password| {
        let username = username.to_string();
        let password = password.to_string();

        let db = state_login.db.lock().unwrap();
        let users = db.users();

        // Find user
        let user = match users.find_by_username(&username) {
            Ok(Some(u)) => u,
            Ok(None) => {
                if let Some(w) = window_weak.upgrade() {
                    w.set_auth_error("User not found".into());
                }
                return;
            }
            Err(e) => {
                if let Some(w) = window_weak.upgrade() {
                    w.set_auth_error(format!("Error: {}", e).into());
                }
                return;
            }
        };

        // Verify password
        let argon2 = Argon2::default();
        let parsed_hash = match PasswordHash::new(&user.password_hash) {
            Ok(h) => h,
            Err(_) => {
                if let Some(w) = window_weak.upgrade() {
                    w.set_auth_error("Invalid stored password".into());
                }
                return;
            }
        };

        if argon2.verify_password(password.as_bytes(), &parsed_hash).is_err() {
            if let Some(w) = window_weak.upgrade() {
                w.set_auth_error("Invalid password".into());
            }
            return;
        }

        // Update last login
        let _ = users.update_last_login(user.id);

        // Create session
        let session = Session::new(user.id, 24 * 7); // 1 week
        if let Err(e) = users.create_session(&session) {
            if let Some(w) = window_weak.upgrade() {
                w.set_auth_error(format!("Session error: {}", e).into());
            }
            return;
        }

        drop(db);

        // Update state
        state_login.set_current_user(Some(user.id));
        state_login.set_current_session(Some(session.id));

        if let Some(w) = window_weak.upgrade() {
            w.set_auth_error("".into());
            w.set_is_logged_in(true);
            w.set_current_username(user.username.into());
        }
    });

    // Register callback
    let state_register = state.clone();
    let window_weak = window.as_weak();
    window.on_register(move |username, password| {
        let username = username.to_string();
        let password = password.to_string();

        if username.len() < 3 {
            if let Some(w) = window_weak.upgrade() {
                w.set_auth_error("Username must be at least 3 characters".into());
            }
            return;
        }

        if password.len() < 6 {
            if let Some(w) = window_weak.upgrade() {
                w.set_auth_error("Password must be at least 6 characters".into());
            }
            return;
        }

        // Hash password
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = match argon2.hash_password(password.as_bytes(), &salt) {
            Ok(h) => h.to_string(),
            Err(_) => {
                if let Some(w) = window_weak.upgrade() {
                    w.set_auth_error("Failed to hash password".into());
                }
                return;
            }
        };

        let user = User::new(username.clone(), password_hash);
        let user_id = user.id;

        let db = state_register.db.lock().unwrap();
        let users = db.users();

        // Check if username exists
        match users.find_by_username(&username) {
            Ok(Some(_)) => {
                if let Some(w) = window_weak.upgrade() {
                    w.set_auth_error("Username already exists".into());
                }
                return;
            }
            Err(e) => {
                if let Some(w) = window_weak.upgrade() {
                    w.set_auth_error(format!("Error: {}", e).into());
                }
                return;
            }
            Ok(None) => {}
        }

        // Create user
        if let Err(e) = users.create(&user) {
            if let Some(w) = window_weak.upgrade() {
                w.set_auth_error(format!("Failed to create user: {}", e).into());
            }
            return;
        }

        // Create session
        let session = Session::new(user_id, 24 * 7);
        if let Err(e) = users.create_session(&session) {
            if let Some(w) = window_weak.upgrade() {
                w.set_auth_error(format!("Session error: {}", e).into());
            }
            return;
        }

        drop(db);

        // Update state
        state_register.set_current_user(Some(user_id));
        state_register.set_current_session(Some(session.id));

        if let Some(w) = window_weak.upgrade() {
            w.set_auth_error("".into());
            w.set_is_logged_in(true);
            w.set_current_username(username.into());
        }
    });

    // Logout callback
    let state_logout = state.clone();
    let window_weak = window.as_weak();
    window.on_logout(move || {
        if let Some(session_id) = state_logout.current_session_id() {
            let db = state_logout.db.lock().unwrap();
            let _ = db.users().delete_session(session_id);
        }

        state_logout.set_current_user(None);
        state_logout.set_current_session(None);
        state_logout.set_current_hall(None);

        if let Some(w) = window_weak.upgrade() {
            w.set_is_logged_in(false);
            w.set_current_username("".into());
        }
    });
}

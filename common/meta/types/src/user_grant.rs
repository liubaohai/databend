// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt;

use common_exception::Result;
use enumflags2::BitFlags;

use crate::UserPrivilegeSet;
use crate::UserPrivilegeType;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum GrantObject {
    Global,
    Database(String),
    Table(String, String),
}

impl GrantObject {
    /// Some global privileges can not be granted to a database or table, for example, a KILL
    /// statement is meaningless for a table.
    pub fn allow_privilege(&self, privilege: UserPrivilegeType) -> bool {
        self.available_privileges().has_privilege(privilege)
    }

    /// Global, database and table has different available privileges
    pub fn available_privileges(&self) -> UserPrivilegeSet {
        match self {
            GrantObject::Global => UserPrivilegeSet::available_privileges_on_global(),
            GrantObject::Database(_) => UserPrivilegeSet::available_privileges_on_database(),
            GrantObject::Table(_, _) => UserPrivilegeSet::available_privileges_on_table(),
        }
    }

    /// Check if there's any privilege which can not be granted to this GrantObject
    pub fn validate_privileges(&self, privileges: UserPrivilegeSet) -> Result<()> {
        let ok = BitFlags::from(privileges)
            .iter()
            .all(|p| self.allow_privilege(p));
        if !ok {
            return Err(common_exception::ErrorCode::IllegalGrant("Illegal GRANT/REVOKE command; please consult the manual to see which privileges can be used"));
        }
        Ok(())
    }
}

impl fmt::Display for GrantObject {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        match self {
            GrantObject::Global => write!(f, "*.*"),
            GrantObject::Database(ref db) => write!(f, "'{}'.*", db),
            GrantObject::Table(ref db, ref table) => write!(f, "'{}'.'{}'", db, table),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq)]
pub struct GrantEntry {
    user: String,
    host_pattern: String,
    object: GrantObject,
    privileges: BitFlags<UserPrivilegeType>,
}

impl GrantEntry {
    pub fn new(
        user: String,
        host_pattern: String,
        object: GrantObject,
        privileges: BitFlags<UserPrivilegeType>,
    ) -> Self {
        Self {
            user,
            host_pattern,
            object,
            privileges,
        }
    }

    pub fn verify_global_privilege(
        &self,
        user: &str,
        host: &str,
        privilege: UserPrivilegeType,
    ) -> bool {
        if !self.matches_user_host(user, host) {
            return false;
        }

        if self.object != GrantObject::Global {
            return false;
        }

        self.privileges.contains(privilege)
    }

    pub fn verify_database_privilege(
        &self,
        user: &str,
        host: &str,
        db: &str,
        privilege: UserPrivilegeType,
    ) -> bool {
        if !self.matches_user_host(user, host) {
            return false;
        }

        if !match &self.object {
            GrantObject::Global => true,
            GrantObject::Database(ref expected_db) => expected_db == db,
            _ => false,
        } {
            return false;
        }

        self.privileges.contains(privilege)
    }

    pub fn verify_table_privilege(
        &self,
        user: &str,
        host: &str,
        db: &str,
        table: &str,
        privilege: UserPrivilegeType,
    ) -> bool {
        if !self.matches_user_host(user, host) {
            return false;
        }

        if !match &self.object {
            GrantObject::Global => true,
            GrantObject::Database(ref expected_db) => expected_db == db,
            GrantObject::Table(ref expected_db, ref expected_table) => {
                expected_db == db && expected_table == table
            }
        } {
            return false;
        }

        self.privileges.contains(privilege)
    }

    pub fn matches_entry(&self, user: &str, host_pattern: &str, object: &GrantObject) -> bool {
        self.user == user && self.host_pattern == host_pattern && &self.object == object
    }

    fn matches_user_host(&self, user: &str, host: &str) -> bool {
        self.user == user && Self::match_host_pattern(&self.host_pattern, host)
    }

    fn match_host_pattern(host_pattern: &str, host: &str) -> bool {
        // TODO: support IP pattern like 0.2.%.%
        if host_pattern == "%" {
            return true;
        }
        host_pattern == host
    }

    fn has_all_available_privileges(&self) -> bool {
        let all_available_privileges = self.object.available_privileges();
        self.privileges
            .contains(BitFlags::from(all_available_privileges))
    }
}

impl fmt::Display for GrantEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        let privileges: UserPrivilegeSet = self.privileges.into();
        let privileges_str = if self.has_all_available_privileges() {
            "ALL".to_string()
        } else {
            privileges.to_string()
        };
        write!(
            f,
            "GRANT {} ON {} TO '{}'@'{}'",
            &privileges_str, self.object, self.user, self.host_pattern
        )
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Eq, PartialEq, Default)]
pub struct UserGrantSet {
    grants: Vec<GrantEntry>,
}

impl UserGrantSet {
    pub fn empty() -> Self {
        Self { grants: vec![] }
    }

    pub fn entries(&self) -> &[GrantEntry] {
        &self.grants
    }

    pub fn verify_global_privilege(
        &self,
        user: &str,
        host: &str,
        privilege: UserPrivilegeType,
    ) -> bool {
        self.grants
            .iter()
            .any(|e| e.verify_global_privilege(user, host, privilege))
    }

    pub fn verify_database_privilege(
        &self,
        user: &str,
        host: &str,
        db: &str,
        privilege: UserPrivilegeType,
    ) -> bool {
        self.grants
            .iter()
            .any(|e| e.verify_database_privilege(user, host, db, privilege))
    }

    pub fn verify_table_privilege(
        &self,
        user: &str,
        host: &str,
        db: &str,
        table: &str,
        privilege: UserPrivilegeType,
    ) -> bool {
        self.grants
            .iter()
            .any(|e| e.verify_table_privilege(user, host, db, table, privilege))
    }

    pub fn grant_privileges(
        &mut self,
        user: &str,
        host_pattern: &str,
        object: &GrantObject,
        privileges: UserPrivilegeSet,
    ) {
        let privileges: BitFlags<UserPrivilegeType> = privileges.into();
        let mut new_grants: Vec<GrantEntry> = vec![];
        let mut changed = false;

        for grant in self.grants.iter() {
            let mut grant = grant.clone();
            if grant.matches_entry(user, host_pattern, object) {
                grant.privileges |= privileges;
                changed = true;
            }
            new_grants.push(grant);
        }

        if !changed {
            new_grants.push(GrantEntry::new(
                user.into(),
                host_pattern.into(),
                object.clone(),
                privileges,
            ))
        }

        self.grants = new_grants;
    }

    pub fn revoke_privileges(
        &mut self,
        user: &str,
        host_pattern: &str,
        object: &GrantObject,
        privileges: UserPrivilegeSet,
    ) {
        let privileges: BitFlags<UserPrivilegeType> = privileges.into();
        let grants = self
            .grants
            .iter()
            .map(|e| {
                if e.matches_entry(user, host_pattern, object) {
                    let mut e = e.clone();
                    e.privileges ^= privileges;
                    e
                } else {
                    e.clone()
                }
            })
            .filter(|e| e.privileges != BitFlags::empty())
            .collect::<Vec<_>>();
        self.grants = grants;
    }
}

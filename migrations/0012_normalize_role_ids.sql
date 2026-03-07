-- 0012_normalize_role_ids.sql
--
-- Rewrite legacy role IDs without relying on foreign key toggles.
-- Handles any existing 32-char hex role IDs by converting them to
-- canonical dashed UUID formatting (8-4-4-4-12), then remaps user_roles
-- and swaps the normalized tables into place.

DROP TABLE IF EXISTS legacy_roles_map;
CREATE TEMP TABLE legacy_roles_map (
    legacy_id TEXT PRIMARY KEY,
    normalized_id TEXT NOT NULL
);

INSERT INTO legacy_roles_map (legacy_id, normalized_id)
SELECT
    id AS legacy_id,
    lower(
        substr(id, 1, 8) || '-' ||
        substr(id, 9, 4) || '-' ||
        substr(id, 13, 4) || '-' ||
        substr(id, 17, 4) || '-' ||
        substr(id, 21, 12)
    ) AS normalized_id
FROM roles
WHERE length(id) = 32
  AND id NOT LIKE '%-%'
  AND lower(id) GLOB '[0-9a-f]*';

INSERT OR IGNORE INTO legacy_roles_map (legacy_id, normalized_id)
SELECT
    ur.role_id AS legacy_id,
    lower(
        substr(ur.role_id, 1, 8) || '-' ||
        substr(ur.role_id, 9, 4) || '-' ||
        substr(ur.role_id, 13, 4) || '-' ||
        substr(ur.role_id, 17, 4) || '-' ||
        substr(ur.role_id, 21, 12)
    ) AS normalized_id
FROM user_roles ur
WHERE length(ur.role_id) = 32
  AND ur.role_id NOT LIKE '%-%'
  AND lower(ur.role_id) GLOB '[0-9a-f]*'
  AND NOT EXISTS (SELECT 1 FROM roles r WHERE r.id = ur.role_id)
;

DROP TABLE IF EXISTS user_roles_new;
DROP TABLE IF EXISTS roles_new;

CREATE TABLE roles_new (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    permissions INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

INSERT INTO roles_new (id, name, permissions, created_at)
SELECT
    COALESCE(
        (SELECT normalized_id FROM legacy_roles_map WHERE legacy_id = r.id),
        r.id
    ) AS id,
    r.name,
    r.permissions,
    r.created_at
FROM roles r;

CREATE TABLE user_roles_new (
    user_id TEXT NOT NULL,
    role_id TEXT NOT NULL,
    PRIMARY KEY (user_id, role_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO user_roles_new (user_id, role_id)
SELECT
    ur.user_id,
    COALESCE(
        (SELECT normalized_id FROM legacy_roles_map WHERE legacy_id = ur.role_id),
        ur.role_id
    ) AS role_id
FROM user_roles ur
WHERE COALESCE(
        (SELECT normalized_id FROM legacy_roles_map WHERE legacy_id = ur.role_id),
        ur.role_id
    ) IN (SELECT id FROM roles_new);

DROP TABLE user_roles;
DROP TABLE roles;
ALTER TABLE roles_new RENAME TO roles;
ALTER TABLE user_roles_new RENAME TO user_roles;

CREATE TABLE user_roles_final (
    user_id TEXT NOT NULL,
    role_id TEXT NOT NULL,
    PRIMARY KEY (user_id, role_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO user_roles_final (user_id, role_id)
SELECT ur.user_id, ur.role_id
FROM user_roles ur
WHERE ur.role_id IN (SELECT id FROM roles);

DROP TABLE user_roles;
ALTER TABLE user_roles_final RENAME TO user_roles;

DROP TABLE legacy_roles_map;

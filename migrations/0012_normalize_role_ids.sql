-- 0012_normalize_role_ids.sql

-- Convert legacy role IDs stored as 32-char hex strings into
-- canonical dashed UUID form, including all existing assignments.

CREATE TEMP TABLE IF NOT EXISTS legacy_roles_map (
    legacy_id TEXT PRIMARY KEY,
    normalized_id TEXT NOT NULL
);

DELETE FROM legacy_roles_map;

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

-- Rebuild user_roles without violating foreign keys while roles are updated.
ALTER TABLE user_roles RENAME TO user_roles_old;

UPDATE roles
SET id = (
    SELECT normalized_id
    FROM legacy_roles_map
    WHERE legacy_id = roles.id
)
WHERE id IN (SELECT legacy_id FROM legacy_roles_map);

CREATE TABLE user_roles (
    user_id TEXT NOT NULL,
    role_id TEXT NOT NULL,
    PRIMARY KEY (user_id, role_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (role_id) REFERENCES roles(id) ON DELETE CASCADE
);

INSERT INTO user_roles (user_id, role_id)
SELECT
    user_id,
    COALESCE(
        (SELECT normalized_id FROM legacy_roles_map WHERE legacy_id = role_id),
        role_id
    ) AS role_id
FROM user_roles_old;

DROP TABLE user_roles_old;
DROP TABLE legacy_roles_map;

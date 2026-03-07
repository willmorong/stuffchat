-- 0012_normalize_role_ids.sql

-- Convert legacy role IDs stored as 32-char hex strings into
-- canonical dashed UUID form, including all existing assignments.

PRAGMA foreign_keys = OFF;

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

UPDATE user_roles
SET role_id = (
    SELECT normalized_id
    FROM legacy_roles_map
    WHERE legacy_id = user_roles.role_id
)
WHERE role_id IN (SELECT legacy_id FROM legacy_roles_map);

UPDATE roles
SET id = (
    SELECT normalized_id
    FROM legacy_roles_map
    WHERE legacy_id = roles.id
)
WHERE id IN (SELECT legacy_id FROM legacy_roles_map);

PRAGMA foreign_keys = ON;
DROP TABLE legacy_roles_map;

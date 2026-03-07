-- 0011_role_capabilities_seed.sql

-- Ensure baseline roles exist and always include the expected capability bits.
INSERT INTO roles(id, name, permissions, created_at)
SELECT LOWER(HEX(RANDOMBLOB(16))),
       'admin',
       255,
       CURRENT_TIMESTAMP
WHERE NOT EXISTS (SELECT 1 FROM roles WHERE name = 'admin');

INSERT INTO roles(id, name, permissions, created_at)
SELECT LOWER(HEX(RANDOMBLOB(16))),
       'member',
       252,
       CURRENT_TIMESTAMP
WHERE NOT EXISTS (SELECT 1 FROM roles WHERE name = 'member');

UPDATE roles
SET permissions = permissions | 255
WHERE name = 'admin';

UPDATE roles
SET permissions = permissions | 252
WHERE name = 'member';

-- Assign the member role to users with no roles.
INSERT OR IGNORE INTO user_roles(user_id, role_id)
SELECT u.id, (SELECT id FROM roles WHERE name = 'member')
FROM users u
WHERE NOT EXISTS (
    SELECT 1 FROM user_roles ur WHERE ur.user_id = u.id
);

-- active users
SELECT id, name FROM users
WHERE created > '2026-01-01' AND active = 1
ORDER BY name;

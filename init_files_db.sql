CREATE TABLE `files` (
	`id` uuid NOT NULL,
	`path` text NOT NULL,
	`downloads` int(11) NOT NULL,
	`last_update` timestamp NOT NULL DEFAULT current_timestamp(),
	PRIMARY KEY (`id`),
	UNIQUE KEY `id` (`id`),
	UNIQUE KEY `path` (`path`) USING HASH
);

CREATE TABLE `users` (
	`username` varchar(16) NOT NULL,
	`password` varchar(255) NOT NULL,
	`email` varchar(32) DEFAULT NULL,
	`perms` int(2) NOT NULL DEFAULT 1,
	`mirror_settings` text CHARACTER SET utf8mb4 COLLATE utf8mb4_bin DEFAULT NULL,
	`registered_at` timestamp NULL DEFAULT NULL,
	`verified` tinyint(1) DEFAULT 1,
	`verification_token` varchar(255) DEFAULT NULL,
	PRIMARY KEY (`username`),
	UNIQUE KEY `username` (`username`)
)

CREATE TABLE `logins` (
	`id` int(11) NOT NULL AUTO_INCREMENT,
	`account` varchar(16) NOT NULL,
	`time` timestamp NOT NULL,
	`ip` varchar(255) NOT NULL,
	`via` varchar(255) NOT NULL DEFAULT 'service',
	PRIMARY KEY (`id`),
	KEY `account` (`account`),
	CONSTRAINT `account` FOREIGN KEY (`account`) REFERENCES `users` (`username`) ON DELETE CASCADE ON UPDATE CASCADE 
);

CREATE TABLE `sessions` (
	`id` varchar(255) NOT NULL,
	`user` varchar(32) NOT NULL,
	`created_at` timestamp NOT NULL DEFAULT current_timestamp(),
	`name` varchar(255) DEFAULT NULL,
	`api_key` tinyint(1) DEFAULT 0,
	PRIMARY KEY (`id`),
	KEY `account` (`user`),
	CONSTRAINT `account` FOREIGN KEY (`user`) REFERENCES `users` (`username`) ON DELETE CASCADE ON UPDATE CASCADE
);

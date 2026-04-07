CREATE TABLE IF NOT EXISTS appdb.widgets (
  id INT PRIMARY KEY AUTO_INCREMENT,
  name VARCHAR(255) NOT NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO appdb.widgets (name)
VALUES ('alpha'), ('beta'), ('gamma');

ALTER USER 'appuser'@'%' REQUIRE X509;

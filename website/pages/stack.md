[⟵ Back](../../README.md#stack)

We’ll build it mostly in Rust with a bit of Java (Spark and Flink) and Python (Airflow) and deployed on AWS and Kubernetes.

| Scope | Solution |
| ----- | -------- |
| REST API, gRPC | axum, tonic |
| Websocket | tokio-tungstenite |
| DB | RDS, ScyllaDB, Google Cloud Spanner, Neo4j |
| Local app DB | SurrealDB |
| Browser app | egui, wasi, wasm-bindgen, rencfs, pgp |
| Local app desktop | egui, mainline, transmission_rs, cratetorrent/rqbit, <br/> quinn, rencfs, pgp, fuse3 |
| Local app mobile | Kotlin Multiplatform |
| Sync daemon | tokio, rencfs, gdrive-rs, git2, rclone, Flink |
| Use Kafka | rdkafka |
| Keycloak | axum-keycloak-auth (in app token verificaton) <br/> or Keycloak Gatekeeper (reverse proxy in front of the services) |
| Event Bus | Kafka / Amazon SQS / RabbitMQ |
| Streaming processor | Flink |
| File storage | S3 |
| Search and Analytics | ELK, Apache Spark, Apache Flink, Apache Airflow |
| Identity Provider | Keycloak |
| Cache | Redis |
| Deploy | AWS Lambda and Amazon EKS |
| Metrics | Prometheus and Grafana Mimir |
| Tracing | Prometheus and Grafana Tempo |
| Logs | Grafana Loki |

---
title: 基于快照预热 Maven 缓存的 Java Spring Boot 开发测试沙箱
author: Sam
date: 2026-07-05
tags:
  - java
  - spring-boot
  - backend
  - maven
  - snapshot
lang: zh-CN
---

# 基于快照预热 Maven 缓存的 Java Spring Boot 开发测试沙箱

## 业务背景

企业后端团队经常需要一次性的 Java 环境来做 API 调试、契约测试、回归复现，以及由
Agent 驱动的代码修改。一个真正有用的沙箱不应该只是能执行 `java -version`，还应该能运行
真实 HTTP 服务、保留工作区状态，并避免每次创建新环境都重新支付 Maven 依赖下载成本。

`examples/java-springboot-web` 模板展示了这一工作流：模板在受控镜像构建阶段预热 Maven 依赖。
demo 随后执行离线构建，启动 Spring Boot 服务，通过 CubeSandbox 路由访问服务，创建任务状态，
为工作区创建快照，再 fork 一个新沙箱来复用构建产物和服务数据。

## 关键挑战

- Java 项目第一次运行前常常需要下载大量 Maven 依赖。
- 后端调试需要真实 HTTP 服务，而不是只有语言运行时。
- 复现 bug 或把工作交给另一个 Agent 时，通常需要同时保留构建产物和工作区状态。
- 企业集群可能限制直接访问公网 Maven 仓库。
- 示例需要足够小，便于 reviewer 和用户跑通。

## CubeSandbox 方案

模板基于 `ghcr.io/tencentcloud/cubesandbox-base:latest` 构建，并安装 Java 21、Maven、curl
和 bash。示例 Spring Boot 项目暴露四个端点：

- `GET /health`
- `GET /api/info`
- `POST /api/tasks`
- `GET /api/tasks/{id}`

任务 API 会把状态持久化到 `/tmp/cubesandbox-spring/state/tasks.json`。demo 脚本执行以下流程：

1. 基于 Java/Spring Boot 模板创建沙箱。
2. 上传 Spring Boot 项目。
3. 使用模板预热的 `/workspace/.m2/repository` 执行
   `mvn --offline -DskipTests package`，构建 `target/*.jar`。
4. 启动服务，通过 CubeSandbox 的 `8080` 端口路由访问它。
5. 创建任务，并验证状态文件存在。
6. 停止服务，创建 CubeSandbox checkpoint snapshot。
7. 从 checkpoint fork 一个新沙箱。
8. 验证 fork 后继承了 Maven cache、构建好的 jar 和任务状态。
9. 直接从继承的 jar 启动 Spring Boot，并读取原始任务。
10. 下载 manifest，证明 cache、artifact、state 和路由访问都已验证。

这展示的是一个可复用的 JVM 后端开发环境，而不是普通 Java 运行时镜像。

## 收益

- 为 Java/Spring Boot 用户提供真实后端服务起点。
- 展示依赖较重项目的模板预热 Maven 缓存和快照保留构建产物模式。
- 展示 fork 沙箱中的有状态工作区继承。
- 使用 CubeProxy 提供的常规 HTTP 服务路由。
- 产出一个小型 JSON manifest，方便 reviewer 检查运行结果。
- v1 不引入外部数据库和多容器编排，保持范围聚焦。

## 受限出口场景

Dockerfile 在模板构建阶段完成 Maven 依赖预热，运行时 demo 使用 Maven offline 模式。生产或
受限出口集群可以用以下三种方式适配：

- 在模板镜像中预先下载依赖。
- 配置 Maven 使用内部仓库镜像。
- 在受控网络环境完成依赖预热，把缓存保存在模板中，后续通过 snapshot/fork 作为重复测试起点。

checkpoint 创建后，fork 出来的沙箱可以直接使用继承的构建产物，不需要为重复服务运行重新下载依赖。

## 参考

- 相关示例：`examples/java-springboot-web`
- 相关 issue：[CubeSandbox 沙箱模板与示例生态](https://github.com/TencentCloud/CubeSandbox/issues/645)

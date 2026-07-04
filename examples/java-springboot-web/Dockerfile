# syntax=docker/dockerfile:1.7
#
# Cube-ready Java/Spring Boot dev/test sandbox image.
# Build:
#   docker build -t cubesandbox-java-springboot-web:latest examples/java-springboot-web
#

ARG CUBE_BASE_IMAGE=ghcr.io/tencentcloud/cubesandbox-base:latest
ARG MAVEN_IMAGE=maven:3.9.9-eclipse-temurin-21
FROM ${MAVEN_IMAGE} AS java-toolchain

FROM ${CUBE_BASE_IMAGE}

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    ca-certificates \
    curl \
    procps \
    && rm -rf /var/lib/apt/lists/*

COPY --from=java-toolchain /opt/java/openjdk /opt/java/openjdk
COPY --from=java-toolchain /usr/share/maven /usr/share/maven

RUN ln -s /usr/share/maven/bin/mvn /usr/bin/mvn

ENV JAVA_HOME=/opt/java/openjdk
ENV MAVEN_HOME=/usr/share/maven
ENV PATH="${JAVA_HOME}/bin:${MAVEN_HOME}/bin:${PATH}"
ENV MAVEN_OPTS="-Dmaven.repo.local=/workspace/.m2/repository"
ENV JAVA_TOOL_OPTIONS="-XX:+UseContainerSupport -XX:MaxRAMPercentage=75"

RUN mkdir -p /workspace/java-springboot-web /workspace/.m2/repository && \
    chmod -R 0777 /workspace

WORKDIR /workspace/java-springboot-web

COPY pom.xml ./
RUN mvn -B -DskipTests dependency:go-offline

COPY src ./src
RUN chown -R user:user /workspace && chmod -R 0777 /workspace

EXPOSE 49983 8080

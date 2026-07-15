# syntax=docker/dockerfile:1.7@sha256:a57df69d0ea827fb7266491f2813635de6f17269be881f696fbfdf2d83dda33e
#
# Cube-ready Java/Spring Boot dev/test sandbox image.
# Build:
#   docker build -t cubesandbox-java-springboot-web:latest examples/java-springboot-web
#

ARG CUBE_BASE_IMAGE=ghcr.io/tencentcloud/cubesandbox-base:latest@sha256:34ea312a63a5534e66ab17005c23d7fbaf33c38bccd5411ee402d901e63a3193
ARG MAVEN_IMAGE=maven:3.9.9-eclipse-temurin-21@sha256:3a4ab3276a087bf276f79cae96b1af04f53731bec53fb2e651aca79e4b10211e
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
ENV SPRING_BOOT_JAVA_OPTS="-XX:+UseContainerSupport -XX:MaxRAMPercentage=75"

RUN mkdir -p /workspace/java-springboot-web /workspace/.m2/repository && \
    chown -R user:user /workspace && \
    find /workspace -type d -exec chmod 0755 {} \; && \
    find /workspace -type f -exec chmod 0644 {} \;

WORKDIR /workspace/java-springboot-web

COPY pom.xml ./
RUN --mount=type=cache,id=java-springboot-maven,target=/tmp/maven-cache \
    --mount=type=secret,id=maven_settings,target=/run/secrets/maven-settings.xml \
    if test -s /run/secrets/maven-settings.xml; then \
        MAVEN_OPTS="-Dmaven.repo.local=/tmp/maven-cache" \
            mvn -s /run/secrets/maven-settings.xml -B \
                -Dmaven.artifact.threads=1 -Dmaven.wagon.http.retryHandler.count=5 \
                -DskipTests dependency:go-offline; \
    else \
        MAVEN_OPTS="-Dmaven.repo.local=/tmp/maven-cache" \
            mvn -B -Dmaven.artifact.threads=1 -Dmaven.wagon.http.retryHandler.count=5 \
                -DskipTests dependency:go-offline; \
    fi && \
    cp -a /tmp/maven-cache/. /workspace/.m2/repository/

COPY src ./src
RUN --mount=type=cache,id=java-springboot-maven,target=/tmp/maven-cache \
    --mount=type=secret,id=maven_settings,target=/run/secrets/maven-settings.xml \
    cp -a /workspace/.m2/repository/. /tmp/maven-cache/ && \
    if test -s /run/secrets/maven-settings.xml; then \
        MAVEN_OPTS="-Dmaven.repo.local=/tmp/maven-cache" \
            mvn -s /run/secrets/maven-settings.xml -B verify; \
    else \
        MAVEN_OPTS="-Dmaven.repo.local=/tmp/maven-cache" mvn -B verify; \
    fi && \
    cp -a /tmp/maven-cache/. /workspace/.m2/repository/ && \
    rm -rf target

RUN chown -R user:user /workspace && \
    touch /var/log/envd.log && \
    chown user:user /var/log/envd.log && \
    find /workspace -type d -exec chmod 0755 {} \; && \
    find /workspace -type f -exec chmod 0644 {} \;

USER user:user

EXPOSE 49983 8080

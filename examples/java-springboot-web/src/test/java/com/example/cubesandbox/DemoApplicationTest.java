package com.example.cubesandbox;

import java.nio.file.Path;
import java.util.UUID;
import org.junit.jupiter.api.Test;
import org.springframework.boot.test.context.SpringBootTest;
import org.springframework.test.context.DynamicPropertyRegistry;
import org.springframework.test.context.DynamicPropertySource;

@SpringBootTest
class DemoApplicationTest {
    private static final Path STATE_DIR = Path.of(
            System.getProperty("java.io.tmpdir"), "cubesandbox-spring-context-" + UUID.randomUUID());

    @DynamicPropertySource
    static void taskStateDirectory(DynamicPropertyRegistry registry) {
        registry.add("cubesandbox.task-state-dir", STATE_DIR::toString);
    }

    @Test
    void contextLoads() {}
}

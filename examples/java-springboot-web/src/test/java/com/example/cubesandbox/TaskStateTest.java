package com.example.cubesandbox;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.Map;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import org.springframework.http.HttpStatus;
import org.springframework.web.server.ResponseStatusException;

class TaskStateTest {
    @TempDir
    Path tempDir;

    private final ObjectMapper objectMapper = new ObjectMapper();

    @Test
    void createTaskPersistsStateAndCanReadItBack() throws Exception {
        TaskState taskState = new TaskState(objectMapper, tempDir);

        Map<String, Object> created = taskState.createTask(Map.of("title", "ship demo"));
        Map<String, Object> loaded = taskState.getTask((String) created.get("id"));

        assertThat(loaded).containsEntry("title", "ship demo");
        assertThat(loaded.get("stateFile")).isEqualTo(tempDir.resolve("tasks.json").toString());
        assertThat(Files.exists(tempDir.resolve("tasks.json"))).isTrue();
    }

    @Test
    void getTaskReturnsNotFoundForMissingId() {
        TaskState taskState = new TaskState(objectMapper, tempDir);

        assertThatThrownBy(() -> taskState.getTask("missing"))
                .isInstanceOfSatisfying(
                        ResponseStatusException.class,
                        ex -> assertThat(ex.getStatusCode()).isEqualTo(HttpStatus.NOT_FOUND));
    }

    @Test
    void createTaskPrunesExpiredStateRows() throws Exception {
        Path stateFile = tempDir.resolve("tasks.json");
        Map<String, Map<String, Object>> tasks = new LinkedHashMap<>();
        tasks.put(
                "old",
                new LinkedHashMap<>(
                        Map.of(
                                "id", "old",
                                "title", "old task",
                                "createdAt", Instant.now().minusSeconds(60 * 60 * 24).toString())));
        objectMapper.writeValue(stateFile.toFile(), tasks);

        TaskState taskState = new TaskState(objectMapper, tempDir);
        taskState.createTask(Map.of("title", "new task"));

        Map<String, Object> persisted = objectMapper.readValue(stateFile.toFile(), new TypeReference<>() {});
        assertThat(persisted).doesNotContainKey("old");
        assertThat(persisted).hasSize(1);
    }
}

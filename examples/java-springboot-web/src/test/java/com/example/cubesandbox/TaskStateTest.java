package com.example.cubesandbox;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import com.fasterxml.jackson.core.type.TypeReference;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.Callable;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
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

        Map<String, Object> created =
                taskState.createTask(new CreateTaskRequest("ship demo", Map.of("priority", "high")));
        Map<String, Object> loaded = taskState.getTask((String) created.get("id"));

        assertThat(loaded).containsEntry("title", "ship demo");
        assertThat(loaded.get("payload")).isEqualTo(Map.of("priority", "high"));
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
    void getTaskReturnsNotFoundForExpiredTask() throws Exception {
        writeTasks(Map.of("expired", taskRow("expired", Instant.now().minusSeconds(13 * 60 * 60))));
        TaskState taskState = new TaskState(objectMapper, tempDir);

        assertThatThrownBy(() -> taskState.getTask("expired"))
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
        taskState.createTask(new CreateTaskRequest("new task", null));

        Map<String, Object> persisted = objectMapper.readValue(stateFile.toFile(), new TypeReference<>() {});
        assertThat(persisted).doesNotContainKey("old");
        assertThat(persisted).hasSize(1);
    }

    @Test
    void createTaskEvictsOldestRowAtOneHundredTaskBoundary() throws Exception {
        Map<String, Map<String, Object>> tasks = new LinkedHashMap<>();
        Instant createdAt = Instant.now().minusSeconds(60);
        for (int index = 0; index < 100; index++) {
            String id = "task-%03d".formatted(index);
            tasks.put(id, taskRow(id, createdAt.plusSeconds(index)));
        }
        writeTasks(tasks);

        TaskState taskState = new TaskState(objectMapper, tempDir);
        Map<String, Object> created = taskState.createTask(new CreateTaskRequest("newest", null));

        Map<String, Map<String, Object>> persisted = readTasks();
        assertThat(persisted).hasSize(100);
        assertThat(persisted).doesNotContainKey("task-000");
        assertThat(persisted).containsKeys("task-001", "task-099", (String) created.get("id"));
    }

    @Test
    void concurrentCreatesAndReadsDoNotLoseOrCorruptTasks() throws Exception {
        TaskState taskState = new TaskState(objectMapper, tempDir);
        ExecutorService executor = Executors.newFixedThreadPool(8);
        List<Callable<Map<String, Object>>> operations = new ArrayList<>();
        for (int index = 0; index < 40; index++) {
            int taskNumber = index;
            operations.add(
                    () -> {
                        Map<String, Object> created = taskState.createTask(
                                new CreateTaskRequest("task-" + taskNumber, Map.of("index", taskNumber)));
                        return taskState.getTask((String) created.get("id"));
                    });
        }

        try {
            List<Future<Map<String, Object>>> results = executor.invokeAll(operations);
            for (Future<Map<String, Object>> result : results) {
                assertThat(result.get()).containsEntry("status", "created");
            }
        } finally {
            executor.shutdownNow();
        }

        assertThat(readTasks()).hasSize(40);
    }

    @Test
    void createTaskRejectsOversizedTitleAndPayload() {
        TaskState taskState = new TaskState(objectMapper, tempDir);

        assertThatThrownBy(() -> taskState.createTask(new CreateTaskRequest("x".repeat(201), null)))
                .isInstanceOfSatisfying(
                        ResponseStatusException.class,
                        ex -> assertThat(ex.getStatusCode()).isEqualTo(HttpStatus.BAD_REQUEST));
        assertThatThrownBy(
                        () -> taskState.createTask(
                                new CreateTaskRequest("large payload", Map.of("blob", "x".repeat(17_000)))))
                .isInstanceOfSatisfying(
                        ResponseStatusException.class,
                        ex -> assertThat(ex.getStatusCode()).isEqualTo(HttpStatus.BAD_REQUEST));
    }

    private Map<String, Object> taskRow(String id, Instant createdAt) {
        return new LinkedHashMap<>(
                Map.of("id", id, "title", id, "status", "created", "createdAt", createdAt.toString()));
    }

    private void writeTasks(Map<String, Map<String, Object>> tasks) throws Exception {
        objectMapper.writeValue(tempDir.resolve("tasks.json").toFile(), tasks);
    }

    private Map<String, Map<String, Object>> readTasks() throws Exception {
        return objectMapper.readValue(tempDir.resolve("tasks.json").toFile(), new TypeReference<>() {});
    }
}

package com.example.cubesandbox;

import static org.hamcrest.Matchers.notNullValue;
import static org.mockito.ArgumentMatchers.any;
import static org.mockito.Mockito.when;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.get;
import static org.springframework.test.web.servlet.request.MockMvcRequestBuilders.post;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.jsonPath;
import static org.springframework.test.web.servlet.result.MockMvcResultMatchers.status;

import java.util.Map;
import org.junit.jupiter.api.Test;
import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.boot.test.autoconfigure.web.servlet.WebMvcTest;
import org.springframework.boot.test.mock.mockito.MockBean;
import org.springframework.http.MediaType;
import org.springframework.test.web.servlet.MockMvc;

@WebMvcTest(TaskController.class)
class TaskControllerTest {
    @Autowired
    private MockMvc mockMvc;

    @MockBean
    private TaskState taskState;

    @Test
    void createTaskAcceptsEmptyBody() throws Exception {
        when(taskState.createTask(any(CreateTaskRequest.class)))
                .thenReturn(Map.of("id", "task-1", "title", "demo task", "status", "created"));

        mockMvc.perform(post("/api/tasks").contentType(MediaType.APPLICATION_JSON))
                .andExpect(status().isCreated())
                .andExpect(jsonPath("$.id", notNullValue()))
                .andExpect(jsonPath("$.status").value("created"));
    }

    @Test
    void createTaskRejectsUnknownFields() throws Exception {
        mockMvc.perform(
                        post("/api/tasks")
                                .contentType(MediaType.APPLICATION_JSON)
                                .content("{\"title\":\"demo\",\"unexpected\":true}"))
                .andExpect(status().isBadRequest());
    }

    @Test
    void getTaskReturnsPersistedTask() throws Exception {
        when(taskState.getTask("task-1")).thenReturn(Map.of("id", "task-1", "title", "demo task"));

        mockMvc.perform(get("/api/tasks/task-1"))
                .andExpect(status().isOk())
                .andExpect(jsonPath("$.id").value("task-1"));
    }
}

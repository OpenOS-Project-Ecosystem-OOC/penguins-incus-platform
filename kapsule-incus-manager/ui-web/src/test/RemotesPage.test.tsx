import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { RemotesPage } from "../pages/RemotesPage";

const REMOTES = [
  { name: "local",  url: "unix://",                protocol: "incus" },
  { name: "prod",   url: "https://prod.example.com", protocol: "incus" },
];

function mockFetch(data: unknown = REMOTES, status = 200) {
  vi.spyOn(global, "fetch").mockImplementation(async () =>
    new Response(JSON.stringify(data), {
      status,
      headers: { "Content-Type": "application/json" },
    })
  );
}

afterEach(() => vi.restoreAllMocks());

// ── rendering ─────────────────────────────────────────────────────────────────

it("renders remote rows", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("local"));
  expect(screen.getByText("prod")).toBeInTheDocument();
  expect(screen.getByText("https://prod.example.com")).toBeInTheDocument();
});

it("shows empty state when no remotes", async () => {
  mockFetch([]);
  render(<RemotesPage />);
  await waitFor(() => expect(screen.getByText(/no remotes configured/i)).toBeInTheDocument());
});

it("shows error on fetch failure", async () => {
  vi.spyOn(global, "fetch").mockRejectedValue(new Error("Connection refused"));
  render(<RemotesPage />);
  await waitFor(() => expect(screen.getByText(/connection refused/i)).toBeInTheDocument());
});

// ── activate ──────────────────────────────────────────────────────────────────

it("renders Activate button for each remote", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("local"));
  const activateButtons = screen.getAllByRole("button", { name: /activate/i });
  expect(activateButtons).toHaveLength(2);
});

it("calls PUT activate endpoint on Activate click", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("prod"));
  const [, prodActivate] = screen.getAllByRole("button", { name: /activate/i });
  fireEvent.click(prodActivate);
  await waitFor(() => {
    const calls = (global.fetch as ReturnType<typeof vi.fn>).mock.calls;
    const activateCall = calls.find(([url, opts]: [string, RequestInit]) =>
      url.includes("prod") && url.includes("activate") && opts?.method === "PUT"
    );
    expect(activateCall).toBeDefined();
  });
});

// ── add remote ────────────────────────────────────────────────────────────────

it("opens add form on + Add click", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("local"));
  fireEvent.click(screen.getByRole("button", { name: /\+ add/i }));
  expect(screen.getByText(/name/i)).toBeInTheDocument();
  expect(screen.getByText(/url/i)).toBeInTheDocument();
});

it("shows validation error when name or url is empty", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("local"));
  fireEvent.click(screen.getByRole("button", { name: /\+ add/i }));
  // Click Add without filling in name
  fireEvent.click(screen.getByRole("button", { name: /^add$/i }));
  expect(screen.getByText(/name and url are required/i)).toBeInTheDocument();
});

it("calls POST remotes on valid add", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("local"));
  fireEvent.click(screen.getByRole("button", { name: /\+ add/i }));

  const inputs = screen.getAllByRole("textbox");
  fireEvent.change(inputs[0], { target: { value: "staging" } });
  fireEvent.change(inputs[1], { target: { value: "https://staging.example.com" } });
  fireEvent.click(screen.getByRole("button", { name: /^add$/i }));

  await waitFor(() => {
    const calls = (global.fetch as ReturnType<typeof vi.fn>).mock.calls;
    const postCall = calls.find(([url, opts]: [string, RequestInit]) =>
      url.includes("/api/v1/remotes") && opts?.method === "POST"
    );
    expect(postCall).toBeDefined();
  });
});

// ── remove remote ─────────────────────────────────────────────────────────────

it("does not show Remove button for local remote", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("local"));
  // Only prod row should have Remove
  const removeButtons = screen.getAllByRole("button", { name: /remove/i });
  expect(removeButtons).toHaveLength(1);
});

it("opens confirm dialog on Remove click", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("prod"));
  fireEvent.click(screen.getByRole("button", { name: /remove/i }));
  expect(screen.getByText(/remove remote/i)).toBeInTheDocument();
  // "prod" appears in both the table row and the dialog; use getAllBy.
  expect(screen.getAllByText(/prod/).length).toBeGreaterThan(0);
});

it("calls DELETE on confirm remove", async () => {
  mockFetch();
  render(<RemotesPage />);
  await waitFor(() => screen.getByText("prod"));
  fireEvent.click(screen.getByRole("button", { name: /remove/i }));
  fireEvent.click(screen.getByRole("button", { name: /^remove$/i }));
  await waitFor(() => {
    const calls = (global.fetch as ReturnType<typeof vi.fn>).mock.calls;
    const deleteCall = calls.find(([url, opts]: [string, RequestInit]) =>
      url.includes("prod") && opts?.method === "DELETE"
    );
    expect(deleteCall).toBeDefined();
  });
});

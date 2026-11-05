__attribute__((import_module("ferryx"), import_name("request")))
extern int request(const char *, int, char *, int);
static char response[4096];
static char document[] = "{\"type\":\"pluginRender\",\"view\":\"main\",\"html\":\"<h1>UTC clock</h1><time>00:00:00</time>\"}";

__attribute__((export_name("tick")))
void tick(long long seconds) {
    const int offset = sizeof("{\"type\":\"pluginRender\",\"view\":\"main\",\"html\":\"<h1>UTC clock</h1><time>") - 1;
    int values[3] = { (int)((seconds / 3600) % 24), (int)((seconds / 60) % 60), (int)(seconds % 60) };
    for (int i = 0; i < 3; i++) {
        document[offset + i * 3] = '0' + values[i] / 10;
        document[offset + i * 3 + 1] = '0' + values[i] % 10;
    }
    request(document, sizeof(document) - 1, response, sizeof(response));
}

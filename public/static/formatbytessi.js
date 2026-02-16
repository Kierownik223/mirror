function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "0 B";
    
    var k = 1000;
    var sizes = ["B", "KB", "MB", "GB", "TB"];
    var i = 0;

    while (bytes >= k && i < sizes.length - 1) {
        bytes = bytes / k;
        i++;
    }

    var value = Math.round(bytes * 10) / 10;

    return value + " " + sizes[i];
}

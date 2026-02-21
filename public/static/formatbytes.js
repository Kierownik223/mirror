function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return "0 B";
    
    var k = 1024;
    var sizes = ["B", "KiB", "MiB", "GiB", "TiB"];
    var i = 0;

    while (bytes >= k && i < sizes.length - 1) {
        bytes = bytes / k;
        i++;
    }

    bytes = Math.round(bytes * 10) / 10;

    return bytes + " " + sizes[i];
}

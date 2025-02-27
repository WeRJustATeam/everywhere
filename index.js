const airplay = require('airplay-protocol');

const server = airplay.Server();

server.on('error', (err) => {
    console.error('Error:', err);
});

server.on('listening', () => {
    console.log('AirPlay server is listening on port 7000');
});

server.on('deviceOn', (device) => {
    console.log('Device found:', device.info);
    device.play('http://your-video-url', 0, (err) => {
        if (err) {
            console.error('Error playing:', err);
        }
    });
});

server.start(); 
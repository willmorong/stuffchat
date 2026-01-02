export const store = {
    baseUrl: localStorage.getItem('stuffchat.base_url') || '',
    accessToken: localStorage.getItem('stuffchat.access_token') || '',
    refreshTokenId: localStorage.getItem('stuffchat.refresh_token_id') || '',
    refreshToken: localStorage.getItem('stuffchat.refresh_token') || '',
    user: null,
    ws: null,
    channels: [],
    allUsers: [], // for channel creation modal
    currentChannelId: null,
    messages: new Map(), // channelId -> array of messages (ascending by created_at)
    oldestMessageId: new Map(), // channelId -> oldest id loaded (for pagination)
    users: new Map(), // userId -> { id, username, avatar_file_id, ... }
    members: new Map(), // channelId -> array of user_ids
    presenceCache: new Map(), // userId -> status
    typingTimers: new Map(), // userId -> timeout
    sessionId: null,
    typingUsers: new Set(), // currently typing in current channel
    theme: localStorage.getItem('stuffchat.theme') || 'mysterious',
    // Audio Preferences
    noiseSuppression: localStorage.getItem('stuffchat.noise_suppression') !== 'false',
    echoCancellation: localStorage.getItem('stuffchat.echo_cancellation') === 'true',
    autoGainControl: localStorage.getItem('stuffchat.auto_gain_control') === 'true',
    // WebRTC
    localStream: null,
    pcs: new Map(), // userId -> RTCPeerConnection
    voiceUsers: new Map(), // channelId -> Set of userIds
    volumeMonitors: new Map(), // userId -> VolumeMonitor instance
    callChannelId: null,
    inCall: false,
};

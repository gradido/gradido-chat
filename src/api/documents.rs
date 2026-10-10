//! The GraphQL documents, the same ones the web chat sends
//! (frontend/src/graphql/chat.graphql, chatGroups.graphql, contacts.graphql).
//!
//! Values always travel as variables, never inside the document: the server
//! masks `body`, `title` and the like in its log by variable name.

macro_rules! with_message_fields {
    ($doc:expr) => {
        concat!(
            $doc,
            " fragment m on ChatMessage { id messageUuid conversationId groupUuid",
            " sender { communityUuid gradidoID }",
            " senderUser { communityUuid gradidoID alias avatarColorIndex avatarUpdatedAt }",
            " announcement mine subject body createdAt editedAt deliveryState notify mailState",
            " images { imageUuid width height } forwarded",
            " forwardedFrom { communityUuid gradidoID alias }",
            " replyTo { id messageUuid mine excerpt hasImage senderUser { communityUuid gradidoID alias } } }"
        )
    };
}

macro_rules! with_group_fields {
    ($doc:expr) => {
        concat!(
            $doc,
            " fragment g on ChatGroup { groupUuid conversationId title communityName",
            " createdBy { communityUuid gradidoID alias }",
            " createdAt role joinedAt mutedByMe memberCount unreadMessages lastMessageAt }"
        )
    };
}

pub const VERIFY_LOGIN: &str = "query { verifyLogin { gradidoID communityUuid alias firstName lastName language transfersInChat avatarColorIndex avatar } }";

pub const CONTACTS: &str = "query($pageSize: Int!) { contactList(currentPage: 1, pageSize: $pageSize) { count contacts {
  user { communityUuid communityName gradidoID alias avatarColorIndex avatarUpdatedAt }
  firstAt lastAt bookings favorite homeCommunity origin unreadChatMessages } } }";

pub const ADD_FAVORITE: &str = "mutation($ref: MemberAvatarRefInput!) { addFavorite(ref: $ref) }";
pub const REMOVE_FAVORITE: &str = "mutation($ref: MemberAvatarRefInput!) { removeFavorite(ref: $ref) }";

pub const MEMBER_AVATARS: &str = "query($refs: [MemberAvatarRefInput!]!) { memberAvatars(refs: $refs) { gradidoID communityUuid avatar avatarUpdatedAt } }";

pub const MESSAGES_WITH_MEMBER: &str = with_message_fields!(
    "query($ref: MemberAvatarRefInput!, $before: Int, $limit: Int) {
  chatMessagesWithMember(ref: $ref, before: $before, limit: $limit) { hasMore mutedByMe messages { ...m } } }"
);

pub const NEW_MESSAGES_SINCE: &str = with_message_fields!(
    "query($afterId: Int, $limit: Int, $editedCursor: String) {
  newChatMessagesSince(afterId: $afterId, limit: $limit, editedCursor: $editedCursor) {
    latestId unreadConversations hasMore messages { ...m } edited { ...m } editedCursor } }"
);

pub const MARK_CONVERSATION_READ: &str = "mutation($ref: MemberAvatarRefInput!, $upToMessageId: Int!) {
  markChatConversationRead(ref: $ref, upToMessageId: $upToMessageId) }";

pub const SEND_MESSAGE: &str = with_message_fields!(
    "mutation($ref: MemberAvatarRefInput!, $body: String!, $notify: ChatMessageNotify!, $replyTo: String) {
  sendChatMessage(ref: $ref, body: $body, notify: $notify, replyTo: $replyTo) { ...m } }"
);

pub const SET_CONVERSATION_MUTED: &str = "mutation($ref: MemberAvatarRefInput!, $muted: Boolean!) { setChatConversationMuted(ref: $ref, muted: $muted) }";

pub const MESSAGE_IMAGE: &str = "query($imageUuid: String!) { chatMessageImage(imageUuid: $imageUuid) }";

pub const EDIT_MESSAGE: &str =
    with_message_fields!("mutation($messageUuid: String!, $body: String!) { editChatMessage(messageUuid: $messageUuid, body: $body) { ...m } }");

pub const FORWARD_MESSAGE: &str =
    "mutation($messageUuid: String!, $groupUuids: [String!]!, $members: [MemberAvatarRefInput!]!, $words: String, $alsoByEmail: Boolean!) {
  forwardChatMessage(messageUuid: $messageUuid, groupUuids: $groupUuids, members: $members, words: $words, alsoByEmail: $alsoByEmail) { id messageUuid } }";

pub const GROUPS: &str = with_group_fields!("query { chatGroups { ...g } }");

pub const GROUP_MESSAGES: &str = with_message_fields!(
    "query($groupUuid: String!, $before: Int, $limit: Int) {
  chatGroupMessages(groupUuid: $groupUuid, before: $before, limit: $limit) { hasMore mutedByMe messages { ...m } } }"
);

pub const SEND_GROUP_MESSAGE: &str = with_message_fields!(
    "mutation($groupUuid: String!, $body: String!, $announce: Boolean!, $replyTo: String) {
  sendChatGroupMessage(groupUuid: $groupUuid, body: $body, announce: $announce, replyTo: $replyTo) { ...m } }"
);

pub const MARK_GROUP_READ: &str =
    "mutation($groupUuid: String!, $upToMessageId: Int!) { markChatGroupRead(groupUuid: $groupUuid, upToMessageId: $upToMessageId) }";

pub const SET_GROUP_MUTED: &str = "mutation($groupUuid: String!, $muted: Boolean!) { setChatGroupMuted(groupUuid: $groupUuid, muted: $muted) }";

pub const GROUP_MEMBERS: &str = "query($groupUuid: String!) { chatGroupMembers(groupUuid: $groupUuid) {
  user { communityUuid communityName gradidoID alias avatarColorIndex avatarUpdatedAt } role joinedAt } }";

pub const CREATE_GROUP: &str =
    with_group_fields!("mutation($title: String!, $members: [MemberAvatarRefInput!]!) { createChatGroup(title: $title, members: $members) { ...g } }");

pub const ADD_GROUP_MEMBERS: &str = with_group_fields!(
    "mutation($groupUuid: String!, $members: [MemberAvatarRefInput!]!) { addChatGroupMembers(groupUuid: $groupUuid, members: $members) { ...g } }"
);

pub const REMOVE_GROUP_MEMBER: &str =
    "mutation($groupUuid: String!, $member: MemberAvatarRefInput!) { removeChatGroupMember(groupUuid: $groupUuid, member: $member) }";

pub const LEAVE_GROUP: &str = "mutation($groupUuid: String!) { leaveChatGroup(groupUuid: $groupUuid) }";

pub const SET_GROUP_MODERATOR: &str = "mutation($groupUuid: String!, $member: MemberAvatarRefInput!, $moderator: Boolean!) {
  setChatGroupModerator(groupUuid: $groupUuid, member: $member, moderator: $moderator) }";

pub const RENAME_GROUP: &str =
    with_group_fields!("mutation($groupUuid: String!, $title: String!) { renameChatGroup(groupUuid: $groupUuid, title: $title) { ...g } }");

pub const TRANSFERS: &str = "query($currentPage: Int!, $pageSize: Int!, $counterparty: MemberAvatarRefInput) {
  transactionList(currentPage: $currentPage, pageSize: $pageSize, order: DESC, counterparty: $counterparty) {
    balance { count }
    transactions { id typeId amount balanceDate memo linkId
      greeting { motif line hasPicture } picture { motif hasPicture } } } }";

pub const GREETING_PICTURE: &str = "query($linkId: Int!) { thankYouGreetingPicture(linkId: $linkId) }";
pub const TRANSACTION_PICTURE: &str = "query($transactionId: Int!) { transactionPicture(transactionId: $transactionId) }";

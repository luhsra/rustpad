import {
  Box,
  Button,
  CloseButton,
  Dialog,
  Flex,
  Icon,
  Popover,
  Portal,
  Text,
} from "@chakra-ui/react";
import { VscOrganization } from "react-icons/vsc";

import UserMe, { User } from "./User";
import { type OnlineUser, type Visibility, canAccess } from "./rustpad";

export type FooterProps = {
  currentUser: OnlineUser;
  users: Record<number, OnlineUser>;
  visibility: Visibility;
  onSetVisibility: (visibility: Visibility) => void;

  onLoadSample: () => void;
  onChangeName: (name: string) => void;
  onChangeColor: () => void;
};

function Footer({
  currentUser,
  users,
  visibility,
  onSetVisibility,

  onLoadSample,
  onChangeName,
  onChangeColor,
}: FooterProps) {
  const visibilityOptions: Visibility[] = ["public", "internal", "private"];

  const currentVisibilityOptions = visibilityOptions.filter(
    (option) => option !== visibility && canAccess(currentUser.role, option),
  );

  return (
    <Flex bgColor="#0071c3" color="white" gap={2}>
      <Dialog.Root>
        <Dialog.Trigger asChild>
          <Button variant="outline" size="xs">
            Settings
          </Button>
        </Dialog.Trigger>
        <Portal>
          <Dialog.Backdrop />
          <Dialog.Positioner>
            <Dialog.Content>
              <Dialog.CloseTrigger asChild>
                <CloseButton size="sm" />
              </Dialog.CloseTrigger>
              <Dialog.Header>
                <Dialog.Title>Document Settings</Dialog.Title>
              </Dialog.Header>
              <Dialog.Body>
                {currentVisibilityOptions.length > 0 && (
                  <Text>Change document visibility</Text>
                )}
                {currentVisibilityOptions.map((option) => (
                  <Button
                    key={option}
                    mt={2}
                    onClick={() => onSetVisibility(option)}
                  >
                    {option}
                  </Button>
                ))}

                <Text>Delete this document and load the example code?</Text>
                <Button mt={4} onClick={onLoadSample}>
                  Load Sample
                </Button>
              </Dialog.Body>
              <Dialog.Footer>
                <Dialog.ActionTrigger asChild>
                  <Button variant="outline">Close</Button>
                </Dialog.ActionTrigger>
              </Dialog.Footer>
            </Dialog.Content>
          </Dialog.Positioner>
        </Portal>
      </Dialog.Root>

      <Box flex={1}></Box>

      <Popover.Root>
        <Popover.Trigger asChild>
          <Button size="xs" variant="outline">
            <Icon as={VscOrganization} /> {Object.keys(users).length} online
          </Button>
        </Popover.Trigger>
        <Portal>
          <Popover.Positioner>
            <Popover.Content>
              <Popover.Arrow />
              <Popover.Body>
                {Object.entries(users).map(([id, user]) => (
                  <Box key={id}>
                    <User info={user} />
                  </Box>
                ))}
              </Popover.Body>
            </Popover.Content>
          </Popover.Positioner>
        </Portal>
      </Popover.Root>

      <UserMe
        info={currentUser}
        onChangeName={onChangeName}
        onChangeColor={onChangeColor}
      />
    </Flex>
  );
}

export default Footer;
